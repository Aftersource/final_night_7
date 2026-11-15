//! Tests of the Earth simulator's core (the grid, the codec and the rules have their own; the simulator inside a
//! match is tested in `rules_tests`).

use fn7_model::earth::{EarthInput, EarthState};

use super::codec;
use super::grid::*;
use super::sim::*;
use super::view;

pub const AMAZON: [f64; 2] = [-60.0, -5.0];
pub const JAPAN_TRENCH: [f64; 2] = [143.5, 38.0];
pub const HONSHU: [f64; 2] = [138.7, 35.4];
pub const SAHARA: [f64; 2] = [10.0, 23.0];
pub const OUTBACK: [f64; 2] = [134.0, -24.0];
pub const PACIFIC: [f64; 2] = [-150.0, 0.0];
pub const SIBERIA: [f64; 2] = [100.0, 62.0];
pub const BENGAL: [f64; 2] = [90.0, 24.0];
pub const CONGO: [f64; 2] = [22.0, 0.0];

pub fn spawn_at(kind: u8, at: [f64; 2], energy: u8, tier: u8) -> Spawn {
    let g = grid();
    Spawn { kind, cell: g.index_of(at), lon_e3: (at[0] * 1000.0) as i32, lat_e3: (at[1] * 1000.0) as i32, energy,
            radius: if tier == TIER_MINI { 0 } else { 2 }, owner: 0, cause: CAUSE_CAST, parent: 0, tier }
}

pub fn input_at(beat: u32, kind: u8, at: [f64; 2], energy: u8, tier: u8) -> EarthInput {
    let s = spawn_at(kind, at, energy, tier);
    EarthInput { beat, kind, cell: s.cell, lon_e3: s.lon_e3, lat_e3: s.lat_e3, energy, radius: s.radius, owner: s.owner, cause: s.cause, tier }
}

fn cell(at: [f64; 2]) -> u16 {
    grid().index_of(at)
}

/// The nearest land (or sea) cell to a point, as a place to cast.
pub fn near(at: [f64; 2], land: bool) -> [f64; 2] {
    let g = grid();
    let i = g.disc(g.index_of(at), 4).into_iter().map(|x| x.0).find(|&i| g.land(i) == land).expect("a cell of that kind nearby");
    g.center[i as usize]
}

/// A fresh simulation with `spawns` cast on beat 1, stepped to `beats` at `entropy`.
fn run(seed: u64, spawns: &[Spawn], beats: u32, entropy: u8) -> (Sim, Vec<Report>) {
    let mut sim = Sim::new(seed, Cfg::default());
    let reps = (1..=beats).map(|b| sim.step(b, entropy, if b == 1 { spawns } else { &[] })).collect();
    (sim, reps)
}

fn total(sim: &Sim, f: impl Fn(&Cell) -> u8) -> u32 {
    sim.list.iter().map(|&i| f(sim.c(i)) as u32).sum()
}

/// A heavy script: everyone casting megas near faults, a few minis, Entropy climbing to heat death.
pub fn heavy_inputs(beats: u32) -> (Vec<EarthInput>, Vec<u8>) {
    let spots = [HONSHU, JAPAN_TRENCH, [-72.0, -33.0], [-122.0, 37.5], [100.0, 0.0], [28.0, 39.0], AMAZON, CONGO, BENGAL,
                 [-90.0, 15.0], [175.0, -40.0], [-20.0, 64.0], SIBERIA, [45.0, 35.0], [121.0, 14.0], PACIFIC];
    let kinds = [VOLCANO, EARTHQUAKE, WILDFIRE, FLOOD, METEOR, STORM, SINK_LAND, RAISE_LAND, SWARM, LIGHTNING, FOREST];
    let mut inputs = vec![];
    for b in 1..=beats {
        for k in 0..4u32 {
            let n = b * 4 + k;
            let mut i = input_at(b, kinds[(n * 7 + k) as usize % kinds.len()], spots[(n * 5 + b) as usize % spots.len()], 8 + (n % 3) as u8, TIER_MEGA);
            i.owner = k as u8;
            inputs.push(i);
        }
        inputs.push(input_at(b, kinds[b as usize % kinds.len()], spots[(b * 3) as usize % spots.len()], 4, TIER_MINI));
    }
    let entropy = (1..=beats).map(|b| ((b * 100) / beats.max(1)).min(100) as u8).collect();
    (inputs, entropy)
}

/// Run a script through the stored state, as a match does. Returns the state and each beat's hash.
pub fn play(seed: u64, cfg: Cfg, inputs: &[EarthInput], entropy: &[u8]) -> (EarthState, Vec<u64>) {
    let mut s = EarthState::default();
    let mut hashes = vec![];
    for (k, e) in entropy.iter().enumerate() {
        let b = k as u32 + 1;
        s.inputs.extend(inputs.iter().filter(|i| i.beat == b).copied());
        let (sim, _, _) = Sim::advance(seed, cfg, &mut s, b, *e);
        hashes.push(sim.hash());
    }
    (s, hashes)
}

// ---------------------------------------------------------------------------------------------------------------------
// Determinism, keyframes, forks
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn the_same_inputs_give_the_same_field() {
    let (inputs, entropy) = heavy_inputs(24);
    let (a, ha) = play(7, Cfg::default(), &inputs, &entropy);
    let (b, hb) = play(7, Cfg::default(), &inputs, &entropy);
    assert_eq!(ha, hb);
    assert_eq!(a, b);
    assert!(ha.windows(2).all(|w| w[0] != w[1]), "the field moves every beat");
    // Another seed: another world (the draws differ).
    let (_, hc) = play(8, Cfg::default(), &inputs, &entropy);
    assert_ne!(ha.last(), hc.last());
    // Another order of the same beat's casts: another world too (the order is an input).
    let mut swapped = inputs.clone();
    swapped.swap(0, 1);
    let (_, hd) = play(7, Cfg::default(), &swapped, &entropy);
    assert_ne!(ha[0], hd[0]);
    // The state survives its own JSON exactly, and is small: arrays, one packed string.
    let json = serde_json::to_string(&a).unwrap();
    let back: EarthState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, a);
    assert_eq!(Sim::from_state(7, Cfg::default(), &back).hash(), *ha.last().unwrap());
    assert!(json.contains("\"inputs\":[[1,"), "{}", &json[..200.min(json.len())]);
}

#[test]
fn any_beat_from_a_keyframe_equals_that_beat_from_the_start() {
    let cfg = Cfg { keyframe_beats: 8, ..Cfg::default() };
    let (inputs, entropy) = heavy_inputs(60);
    let (s, hashes) = play(11, cfg, &inputs, &entropy);
    assert_eq!(s.keyframes.iter().map(|k| k.beat).collect::<Vec<_>>(), vec![8, 16, 24, 32, 40, 48, 56]);
    for b in 1..=60u32 {
        let from_kf = Sim::at_beat(11, cfg, &s, b).unwrap();
        let from_start = Sim::at_beat_from(11, cfg, &s, b, false).unwrap();
        // Bit for bit: every stored byte of every cell, every phenomenon, the counters.
        assert_eq!(from_kf.sparse(), from_start.sparse(), "beat {b}");
        assert_eq!(from_kf.phen, from_start.phen, "beat {b}");
        assert_eq!((from_kf.next_id, from_kf.tension, from_kf.beat), (from_start.next_id, from_start.tension, b));
        assert_eq!(from_kf.hash(), hashes[b as usize - 1], "beat {b} is the beat that was lived");
    }
    assert!(Sim::at_beat(11, cfg, &s, 61).is_none(), "the future is not there");
    // Beat 0: before anything.
    assert_eq!(Sim::at_beat(11, cfg, &s, 0).unwrap().list.len(), 0);
    // Another K: other keyframes, the same field.
    let cfg4 = Cfg { keyframe_beats: 3, ..Cfg::default() };
    let (s4, h4) = play(11, cfg4, &inputs, &entropy);
    assert_eq!(h4, hashes);
    assert_eq!(s4.keyframes.len(), 20);
    assert_eq!(Sim::at_beat(11, cfg4, &s4, 37).unwrap().hash(), hashes[36]);
}

#[test]
fn a_fork_continues_identically_and_diverges_only_by_its_own_casts() {
    let cfg = Cfg::default();
    let (inputs, entropy) = heavy_inputs(40);
    let (full, hashes) = play(5, cfg, &inputs, &entropy);
    // Fork at beat 20: the stored state alone (through JSON, as a recording holds it) is enough to go on.
    let (mut at20, _) = play(5, cfg, &inputs, &entropy[..20]);
    at20 = serde_json::from_str(&serde_json::to_string(&at20).unwrap()).unwrap();
    let mut fork = at20.clone();
    for b in 21..=40u32 {
        fork.inputs.extend(inputs.iter().filter(|i| i.beat == b).copied());
        let (sim, _, _) = Sim::advance(5, cfg, &mut fork, b, entropy[b as usize - 1]);
        assert_eq!(sim.hash(), hashes[b as usize - 1], "beat {b}");
    }
    assert_eq!(fork, full);
    // The same fork with one more cast at beat 21 is another world from there on, and the past is untouched.
    let mut other = at20;
    other.inputs.push(input_at(21, METEOR, SAHARA, 10, TIER_MEGA));
    let (sim, _, _) = Sim::advance(5, cfg, &mut other, 21, entropy[20]);
    assert_ne!(sim.hash(), hashes[20]);
    assert_eq!(Sim::at_beat(5, cfg, &other, 20).unwrap().hash(), hashes[19]);
}

// ---------------------------------------------------------------------------------------------------------------------
// Each phenomenon's rule
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn a_storm_rides_the_wind_and_rains() {
    let (sim, _) = run(1, &[spawn_at(STORM, CONGO, 9, TIER_MEGA)], 1, 0);
    let c0 = cell(CONGO);
    assert_eq!(sim.phen[0].kind, STORM);
    assert!(sim.c(c0).cloud >= 40, "cloud {}", sim.c(c0).cloud);
    let (sim, _) = run(1, &[spawn_at(STORM, CONGO, 9, TIER_MEGA)], 5, 0);
    let p = sim.phen[0];
    assert_ne!(p.cell, c0, "it moved");
    assert!(p.dir < 6 && p.power < 9 * 16, "heading {} power {}", p.dir, p.power);
    assert!(total(&sim, |c| c.water) > 0, "it rained on the land");
    assert!(sim.list.iter().any(|&i| sim.c(i).charge > 0), "charge builds under heavy cloud");
    // It ends, and the sky clears.
    let (sim, reps) = run(1, &[spawn_at(STORM, CONGO, 9, TIER_MEGA)], 45, 0);
    assert!(sim.phen.is_empty() && reps.iter().any(|r| r.ended.iter().any(|p| p.kind == STORM)));
    assert_eq!(total(&sim, |c| c.cloud), 0);
}

#[test]
fn lightning_strikes_and_sets_the_forest_alight() {
    let (sim, reps) = run(2, &[spawn_at(LIGHTNING, AMAZON, 8, TIER_MEGA)], 1, 0);
    assert_eq!(reps[0].strikes.len(), 3, "a mega: 1 + magnitude/3 strikes");
    assert_eq!(reps[0].strikes[0], cell(AMAZON));
    assert!(sim.c(cell(AMAZON)).fire > 0);
    // A mini: one strike. On sand nothing burns.
    let (sim, reps) = run(2, &[spawn_at(LIGHTNING, SAHARA, 3, TIER_MINI)], 1, 0);
    assert_eq!(reps[0].strikes, vec![cell(SAHARA)]);
    assert_eq!(total(&sim, |c| c.fire), 0);
}

#[test]
fn a_flood_pours_flows_downhill_and_drains() {
    let (sim, _) = run(3, &[spawn_at(FLOOD, BENGAL, 9, TIER_MEGA)], 3, 0);
    let c0 = cell(BENGAL);
    assert!(sim.c(c0).water >= 48, "water {}", sim.c(c0).water);
    assert_eq!(sim.top(c0), T_FLOOD);
    // Water never stands on a cell higher than a dry neighbour it could run to... it runs: more cells are wet than
    // the flood's own disc, and none of them is sea.
    let wet: Vec<u16> = sim.list.iter().copied().filter(|&i| sim.c(i).water > 0 && !sim.is_sea(i)).collect();
    assert!(wet.len() >= 5, "{} wet cells", wet.len());
    let (sim, _) = run(3, &[spawn_at(FLOOD, BENGAL, 9, TIER_MEGA)], 40, 0);
    assert_eq!(total(&sim, |c| c.water), 0, "it drained");
    assert!(sim.c(c0).ruin > 0 || sim.c(c0).is_base(), "what it ruined fades slowly");
}

#[test]
fn a_wildfire_spreads_on_fuel_and_burns_out() {
    let (sim, reps) = run(4, &[spawn_at(WILDFIRE, AMAZON, 9, TIER_MEGA)], 6, 0);
    let burning = sim.list.iter().filter(|&&i| sim.c(i).fire > 0).count();
    assert!(burning >= 8, "{burning} cells burning");
    assert!(reps.iter().map(|r| r.ignitions).sum::<u32>() >= burning as u32);
    assert!(sim.list.iter().all(|&i| sim.c(i).fire == 0 || !sim.is_sea(i)), "the sea does not burn");
    assert!(sim.fuel(cell(AMAZON)) < grid().fuel[cell(AMAZON) as usize], "it ate its fuel");
    // On sand: scorched ground, no fire.
    let (sim, _) = run(4, &[spawn_at(WILDFIRE, SAHARA, 9, TIER_MEGA)], 2, 0);
    assert_eq!(total(&sim, |c| c.fire), 0);
    // It burns out: with the fuel gone the fire dies and the forest starts back. Borneo is an island.
    let (sim, _) = run(4, &[spawn_at(WILDFIRE, near([114.0, 0.5], true), 9, TIER_MEGA)], 60, 0);
    assert_eq!(total(&sim, |c| c.fire), 0, "Borneo burned out");
    assert!(sim.list.iter().any(|&i| sim.c(i).dfuel < 0));
    // On a continent it is a front that moves, not a sea of fire: bounded at any moment.
    let (sim, _) = run(4, &[spawn_at(WILDFIRE, AMAZON, 9, TIER_MEGA)], 30, 0);
    let burning = sim.list.iter().filter(|&&i| sim.c(i).fire > 0).count();
    assert!(burning <= 120, "{burning} cells burning at beat 30");
}

#[test]
fn a_volcano_builds_a_cone_pours_lava_that_cools_to_rock_and_sleeps() {
    let fuji = near(HONSHU, true);
    let c0 = cell(fuji);
    let h0 = grid().h[c0 as usize];
    let (sim, reps) = run(5, &[spawn_at(VOLCANO, fuji, 10, TIER_MEGA)], 1, 0);
    assert!(sim.height(c0) >= (h0 + 2).min(15), "the cone");
    assert_eq!(reps[0].eruptions, vec![c0]);
    assert!(sim.c(c0).lava > 0 && sim.c(c0).heat > 0);
    assert!(matches!(sim.top(c0), T_LAVA | T_FIRE), "lava at the vent (most of it runs off the cone)");
    assert!(sim.list.iter().any(|&i| i != c0 && sim.c(i).lava > 0), "it flows downhill");
    let (sim, reps) = run(5, &[spawn_at(VOLCANO, fuji, 10, TIER_MEGA)], 12, 0);
    assert!(reps.iter().map(|r| r.rock_made).sum::<u32>() >= 2, "lava cooled to rock");
    assert!(sim.list.iter().any(|&i| i != c0 && sim.c(i).dh > 0), "the flows raised their columns");
    assert!(total(&sim, |c| c.ash) > 0, "ash fell downwind");
    let v = sim.phen.iter().find(|p| p.kind == VOLCANO).unwrap();
    assert_eq!((v.dur, v.power), (0, 0), "dormant");
    let (sim, _) = run(5, &[spawn_at(VOLCANO, fuji, 10, TIER_MEGA)], 30, 0);
    assert!(sim.phen.is_empty());
    assert!(sim.height(c0) > h0, "the mountain stays");
    // A mini: no cone.
    let (sim, _) = run(5, &[spawn_at(VOLCANO, OUTBACK, 6, TIER_MINI)], 1, 0);
    assert!(sim.c(cell(OUTBACK)).lava > 0 && sim.c(cell(OUTBACK)).dh == 0);
}

#[test]
fn an_earthquake_ruptures_along_the_nearest_fault_and_is_mild_far_from_one() {
    let (sim, reps) = run(6, &[spawn_at(EARTHQUAKE, JAPAN_TRENCH, 9, TIER_MEGA)], 1, 0);
    let (_, epi, n) = reps[0].ruptures[0];
    assert_eq!(grid().fault[epi as usize], 0);
    assert_eq!(n, 20, "2 + 2 × magnitude cells");
    let cracked: Vec<u16> = sim.list.iter().copied().filter(|&i| sim.c(i).crack >= 64).collect();
    assert!(cracked.len() >= 10 && cracked.iter().all(|&i| grid().fault[i as usize] <= 1), "{} cracked", cracked.len());
    assert!(reps[0].shaken.iter().any(|x| x.1 == 255));
    assert!(sim.c(epi).stress < 0, "the stress is released");
    // Offshore: a tidal wave; on a boundary and great: the plate moves.
    let kinds: Vec<u8> = sim.phen.iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&TIDAL_WAVE) && kinds.contains(&PLATE_DRIFT), "{kinds:?}");
    assert!(sim.phen.iter().filter(|p| p.kind != EARTHQUAKE).all(|p| p.parent == 1 && p.cause == CAUSE_INTERACTION));
    // Aftershocks.
    let (_, reps) = run(6, &[spawn_at(EARTHQUAKE, JAPAN_TRENCH, 9, TIER_MEGA)], 6, 0);
    assert!(reps[1..].iter().map(|r| r.aftershocks.len()).sum::<usize>() >= 1);
    // Far from any fault: the epicentre alone, a weaker shake, nothing set off.
    let (sim, reps) = run(6, &[spawn_at(EARTHQUAKE, OUTBACK, 9, TIER_MEGA)], 1, 0);
    assert_eq!(reps[0].ruptures[0].2, 1);
    assert_eq!(sim.phen.len(), 1);
    assert!(reps[0].shaken.iter().map(|x| x.1).max().unwrap() < 255);
}

#[test]
fn a_meteor_digs_a_crater_and_offshore_raises_a_wave() {
    let c0 = cell(CONGO);
    let h0 = grid().h[c0 as usize];
    let (sim, reps) = run(7, &[spawn_at(METEOR, CONGO, 10, TIER_MEGA)], 1, 0);
    assert_eq!(sim.height(c0), h0 - 2, "the crater");
    assert_eq!(reps[0].impacts, vec![c0]);
    assert!(sim.c(c0).ruin >= 200 && total(&sim, |c| c.fire) > 0 && total(&sim, |c| c.plume) > 0);
    assert!(sim.phen.iter().all(|p| p.kind == METEOR));
    let (sim, _) = run(7, &[spawn_at(METEOR, PACIFIC, 10, TIER_MEGA)], 1, 0);
    assert!(sim.phen.iter().any(|p| p.kind == TIDAL_WAVE && p.parent == 1));
    assert!(sim.c(cell(PACIFIC)).cloud > 0, "steam");
    // A mini: scorched and cracked, the ground keeps its height.
    let (sim, _) = run(7, &[spawn_at(METEOR, CONGO, 7, TIER_MINI)], 1, 0);
    assert!(sim.c(c0).dh == 0 && sim.c(c0).crack > 0);
}

#[test]
fn a_tidal_wave_radiates_and_breaks_on_the_coast() {
    // From the Japan trench: the front moves a cell per sub-step and floods low coasts.
    let mut sim = Sim::new(8, Cfg::default());
    let origin = *grid().disc(cell(JAPAN_TRENCH), 2).iter().map(|x| &x.0).find(|&&i| sim.is_sea(i)).unwrap();
    let s = Spawn { kind: TIDAL_WAVE, cell: origin, lon_e3: 0, lat_e3: 0, energy: 9, radius: 0, owner: NOBODY, cause: CAUSE_INTERACTION,
                    parent: 0, tier: TIER_WORLD };
    let mut hits = 0;
    let mut fronts = vec![];
    for b in 1..=14 {
        let r = sim.step(b, 0, if b == 1 { std::slice::from_ref(&s) } else { &[] });
        hits += r.wave_hits.len();
        fronts.extend(sim.phen.first().map(|p| (p.a, p.power)));
    }
    assert_eq!(fronts[..3].iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert!(fronts.windows(2).all(|w| w[1].1 < w[0].1), "it weakens as it spreads: {fronts:?}");
    assert!(hits >= 3, "{hits} coastal cells hit");
    assert!(sim.phen.is_empty(), "it is spent");
}

#[test]
fn land_rises_from_the_sea_and_sinks_under_it() {
    // A shelf cell off Bengal rises: new land.
    let shelf = *grid().disc(cell([90.0, 20.0]), 3).iter().map(|x| &x.0).find(|&&i| grid().h[i as usize] == 7).unwrap();
    let s = Spawn { cell: shelf, ..spawn_at(RAISE_LAND, [0.0, 0.0], 9, TIER_MEGA) };
    let mut sim = Sim::new(9, Cfg::default());
    let rose: Vec<u16> = (1..=4).flat_map(|b| sim.step(b, 0, if b == 1 { std::slice::from_ref(&s) } else { &[] }).land_rose).collect();
    assert!(rose.contains(&shelf));
    assert_eq!(sim.height(shelf), 10, "three beats, a level each");
    assert_eq!(TOPS[sim.top(shelf) as usize], "rock");
    // A coastal lowland sinks: the sea takes it.
    let low = *grid().disc(cell(BENGAL), 3).iter().map(|x| &x.0).find(|&&i| grid().h[i as usize] == 8).unwrap();
    let s = Spawn { cell: low, ..spawn_at(SINK_LAND, [0.0, 0.0], 9, TIER_MEGA) };
    let mut sim = Sim::new(9, Cfg::default());
    let r = sim.step(1, 0, std::slice::from_ref(&s));
    assert!(r.drowned.contains(&low));
    assert!(sim.is_sea(low) && sim.top(low) == T_WATER && sim.c(low).ruin == 255);
    // Minis change no heights.
    let (sim, _) = run(9, &[spawn_at(SINK_LAND, CONGO, 5, TIER_MINI), spawn_at(RAISE_LAND, AMAZON, 3, TIER_MINI)], 3, 0);
    assert!(sim.list.iter().all(|&i| sim.c(i).dh == 0));
}

#[test]
fn a_forest_grows_a_swarm_eats_it_and_a_blizzard_buries_the_north() {
    let (sim, _) = run(10, &[spawn_at(FOREST, SAHARA, 8, TIER_MEGA)], 8, 0);
    let c0 = cell(SAHARA);
    assert!(sim.fuel(c0) >= 90, "fuel {}", sim.fuel(c0));
    assert_eq!(TOPS[sim.top(c0) as usize], "forest", "the desert is green");
    // A swarm strips the Congo and moves on.
    let (sim, _) = run(10, &[spawn_at(SWARM, CONGO, 8, TIER_MEGA)], 6, 0);
    assert!(sim.list.iter().filter(|&&i| sim.c(i).dfuel < 0).count() >= 4);
    assert!(sim.phen[0].cell != cell(CONGO) || sim.phen[0].dir == NO_DIR);
    let (sim, _) = run(10, &[spawn_at(SWARM, CONGO, 8, TIER_MEGA)], 60, 0);
    assert_eq!(total(&sim, |c| c.swarm), 0, "it starves or scatters in the end");
    // A storm cast on Siberia is a blizzard: cold, snow.
    let (sim, _) = run(10, &[spawn_at(STORM, SIBERIA, 9, TIER_MEGA)], 4, 0);
    assert_eq!(sim.phen[0].kind, BLIZZARD);
    assert!(total(&sim, |c| c.snow) > 100 && sim.list.iter().any(|&i| sim.c(i).heat < 0));
    assert!(sim.list.iter().any(|&i| TOPS[sim.top(i) as usize] == "snow"));
}

#[test]
fn a_patch_of_plate_drifts_a_rift_behind_and_a_ridge_ahead() {
    let cfg = Cfg { plate_beats: 2, ..Cfg::default() };
    let mut sim = Sim::new(12, cfg);
    let c0 = *grid().fault_cells.iter().find(|&&i| grid().h[i as usize] >= 9 && grid().nb[i as usize].iter().all(|&j| grid().h[j as usize] >= 8)).unwrap();
    let s = Spawn { cell: c0, radius: 1, ..spawn_at(PLATE_DRIFT, [0.0, 0.0], 9, TIER_WORLD) };
    let mut moves = vec![];
    let before: Vec<u8> = grid().disc(c0, 1).iter().map(|x| sim.height(x.0)).collect();
    for b in 1..=12 {
        let r = sim.step(b, 0, if b == 1 { std::slice::from_ref(&s) } else { &[] });
        moves.extend(r.plate_moves.iter().map(|m| (b, m.1)));
        if b == 2 {
            // One move: every column of the patch is one cell along the heading.
            let p = sim.phen[0];
            assert_eq!(p.cell, grid().nb[c0 as usize][p.dir as usize]);
            let after: Vec<u8> = grid().disc(c0, 1).iter().map(|x| sim.height(grid().nb[x.0 as usize][p.dir as usize])).collect();
            assert_eq!(after, before, "the land moved whole");
            let rift: Vec<u16> = sim.list.iter().copied().filter(|&i| sim.c(i).crack == 255).collect();
            assert_eq!(rift.len(), 3, "three cells of rift behind a 7-cell patch");
            assert!(rift.iter().all(|&i| sim.c(i).lava > 0 || sim.is_sea(i)), "the rift leaks magma");
            assert!(TOPS[sim.top(rift[0]) as usize] == "rift" || sim.c(rift[0]).lava >= 8 || sim.is_sea(rift[0]));
        }
    }
    assert_eq!(moves.iter().map(|m| m.0).collect::<Vec<_>>(), vec![2, 4, 6, 8, 10], "a cell every `plate_beats`, 2 + magnitude/3 moves");
    assert!(sim.phen.is_empty());
    assert!(sim.list.iter().filter(|&&i| sim.c(i).crack == 255).count() >= 9, "the rift is as long as the drift");
}

// ---------------------------------------------------------------------------------------------------------------------
// Interactions
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn flood_quenches_fire_into_steam() {
    let mut sim = Sim::new(20, Cfg::default());
    sim.step(1, 0, &[spawn_at(WILDFIRE, AMAZON, 9, TIER_MEGA)]);
    sim.step(2, 0, &[]);
    let burning = sim.list.iter().filter(|&&i| sim.c(i).fire > 0).count();
    assert!(burning >= 3);
    let cloud = total(&sim, |c| c.cloud);
    let r = sim.step(3, 0, &[spawn_at(FLOOD, AMAZON, 10, TIER_MEGA)]);
    let r2 = sim.step(4, 0, &[]);
    assert!(r.quenched + r2.quenched >= 3, "quenched {} + {}", r.quenched, r2.quenched);
    assert_eq!(sim.c(cell(AMAZON)).fire, 0);
    assert!(total(&sim, |c| c.cloud) > cloud + 50, "steam rose");
}

#[test]
fn lava_meeting_water_makes_rock_and_steam() {
    // A volcano on the sea floor: steam, and rock that builds toward the surface.
    let mut sim = Sim::new(21, Cfg::default());
    let c0 = cell(PACIFIC);
    let h0 = sim.height(c0);
    let v = [spawn_at(VOLCANO, PACIFIC, 10, TIER_MEGA)];
    let reps: Vec<Report> = (1..=10).map(|b| sim.step(b, 0, if b == 1 { &v[..] } else { &[] })).collect();
    assert!(reps.iter().map(|r| r.steamed).sum::<u32>() >= 5);
    assert!(sim.height(c0) > h0 + 2, "from {h0} to {}", sim.height(c0));
    assert!(sim.list.iter().any(|&i| sim.c(i).cloud > 0), "steam clouds");
    // On land: a flood over fresh lava cools it at once.
    let fuji = near(HONSHU, true);
    let (dry, _) = run(21, &[spawn_at(VOLCANO, fuji, 10, TIER_MEGA)], 3, 0);
    let mut wet = Sim::new(21, Cfg::default());
    wet.step(1, 0, &[spawn_at(VOLCANO, fuji, 10, TIER_MEGA)]);
    let a = wet.step(2, 0, &[spawn_at(FLOOD, fuji, 10, TIER_MEGA)]);
    let b = wet.step(3, 0, &[]);
    assert!(a.steamed + b.steamed >= 1);
    assert!(total(&wet, |c| c.lava) < total(&dry, |c| c.lava));
}

#[test]
fn a_quake_near_a_volcano_sets_it_off() {
    let mut sim = Sim::new(22, Cfg::default());
    sim.step(1, 0, &[spawn_at(VOLCANO, HONSHU, 6, TIER_MEGA)]);
    for b in 2..=9 {
        sim.step(b, 0, &[]);
    }
    assert_eq!(sim.phen[0].dur, 0, "asleep");
    let r = sim.step(10, 0, &[spawn_at(EARTHQUAKE, HONSHU, 9, TIER_MEGA)]);
    assert_eq!(r.woke, vec![1]);
    let v = sim.phen.iter().find(|p| p.id == 1).unwrap();
    assert!(v.dur >= 1 && v.power > 0, "erupting again: {v:?}");
    assert!(sim.c(v.cell).lava > 0);
    // A quake far away wakes nothing.
    let mut sim = Sim::new(22, Cfg::default());
    sim.step(1, 0, &[spawn_at(VOLCANO, HONSHU, 6, TIER_MEGA)]);
    assert!(sim.step(2, 0, &[spawn_at(EARTHQUAKE, OUTBACK, 9, TIER_MEGA)]).woke.is_empty());
}

#[test]
fn storms_feed_floods_fires_feed_storms_and_ash_cools() {
    // A storm alone leaves standing water where it sat.
    let (sim, _) = run(23, &[spawn_at(STORM, CONGO, 10, TIER_MEGA)], 7, 0);
    assert!(sim.list.iter().any(|&i| sim.c(i).water >= 16), "rain pooled");
    // A fire under the sky makes its own cloud and charge; a storm that crosses a fire grows.
    let (sim, _) = run(23, &[spawn_at(WILDFIRE, AMAZON, 10, TIER_MEGA)], 4, 0);
    assert!(total(&sim, |c| c.cloud) > 0 && total(&sim, |c| c.plume) > 0);
    let alone = run(23, &[spawn_at(STORM, AMAZON, 6, TIER_MEGA)], 3, 0).0.phen[0].power;
    let fed = run(23, &[spawn_at(WILDFIRE, AMAZON, 10, TIER_MEGA), spawn_at(STORM, AMAZON, 6, TIER_MEGA)], 3, 0).0;
    assert!(fed.phen.iter().find(|p| p.kind == STORM).unwrap().power > alone, "the fire fed the storm");
    // Ash in the air cools what is under it: the same hot cell, with and without a plume.
    let mut a = Sim::new(23, Cfg::default());
    let mut b = Sim::new(23, Cfg::default());
    let c0 = cell(SAHARA);
    a.m(c0).heat = 100;
    b.m(c0).heat = 100;
    b.m(c0).plume = 200;
    a.normalize();
    b.normalize();
    a.step(1, 0, &[]);
    b.step(1, 0, &[]);
    assert!(b.c(c0).heat < a.c(c0).heat, "{} < {}", b.c(c0).heat, a.c(c0).heat);
    // Snow melts into water where it is warm; a swarm dies in fire.
    let mut s = Sim::new(23, Cfg::default());
    s.m(cell(CONGO)).snow = 200;
    s.m(cell(AMAZON)).swarm = 200;
    s.m(cell(AMAZON)).fire = 200;
    s.normalize();
    let r = s.step(1, 0, &[]);
    assert!(r.melted >= 1 && s.c(cell(CONGO)).water > 0 && s.c(cell(CONGO)).snow < 200);
    assert!(s.c(cell(AMAZON)).swarm < 100);
}

// ---------------------------------------------------------------------------------------------------------------------
// Entropy
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn entropy_quickens_everything_and_the_world_starts_its_own() {
    let fire = [spawn_at(WILDFIRE, AMAZON, 9, TIER_MEGA)];
    let calm = run(30, &fire, 5, 0).0;
    let wild = run(30, &fire, 5, 90).0;
    let burning = |s: &Sim| s.list.iter().filter(|&&i| s.c(i).fire > 0 || s.c(i).dfuel < 0).count();
    assert!(burning(&wild) > burning(&calm) * 3 / 2, "{} against {}", burning(&wild), burning(&calm));
    // No Entropy: the world starts nothing. Entropy 30: now and then. Entropy 90: every beat or so.
    let own = |e: u8| run(31, &[], 40, e).1.iter().map(|r| r.spontaneous).sum::<u32>();
    let (none, some, many) = (own(0), own(30), own(90));
    assert_eq!(none, 0);
    assert!((1..=20).contains(&some), "{some} at Entropy 30");
    assert!(many >= 25 && many > some * 2, "{many} at Entropy 90");
    // They start on the faults and are the world's own.
    let (sim, _) = run(31, &[], 10, 90);
    assert!(sim.phen.iter().all(|p| p.owner == NOBODY && matches!(p.cause, CAUSE_ENTROPY | CAUSE_INTERACTION)));
    assert!(sim.phen.iter().any(|p| matches!(p.kind, EARTHQUAKE | VOLCANO | PLATE_DRIFT)));
    // More sub-steps as the timeline runs fast.
    let subs = |e: u8| Sim::new(1, Cfg::default()).step(1, e, &[]).substeps;
    assert_eq!((subs(0), subs(59), subs(60), subs(80), subs(100)), (1, 1, 2, 3, 4));
}

#[test]
fn heat_death_cascades() {
    let (inputs, _) = heavy_inputs(6);
    let entropy = [30, 50, 70, 85, 95, 100];
    let mut s = EarthState::default();
    let mut last = None;
    let mut cells = vec![];
    for (k, e) in entropy.iter().enumerate() {
        let b = k as u32 + 1;
        s.inputs.extend(inputs.iter().filter(|i| i.beat == b).copied());
        let before: Vec<i16> = Sim::from_state(3, Cfg::default(), &s).phen.iter().map(|p| p.power).collect();
        let (sim, rep, _) = Sim::advance(3, Cfg::default(), &mut s, b, *e);
        cells.push(sim.list.len());
        last = Some((sim, rep, before));
    }
    let (sim, rep, _) = last.unwrap();
    assert_eq!(rep.substeps, 4);
    assert!(rep.spontaneous >= 5, "the faults let go together: {}", rep.spontaneous);
    assert!(sim.phen.len() >= 20, "{} phenomena at once", sim.phen.len());
    assert!(cells[5] > cells[2] * 2, "the damage runs away: {cells:?}");
    assert!(sim.list.len() <= Cfg::default().max_cells as usize);
}

// ---------------------------------------------------------------------------------------------------------------------
// The caps
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn the_caps_hold_and_what_settles_is_reported() {
    let cfg = Cfg { max_cells: 200, max_phenomena: 8, ..Cfg::default() };
    let (inputs, entropy) = heavy_inputs(30);
    let mut s = EarthState::default();
    let mut evicted = 0;
    let mut refused = 0;
    for (k, e) in entropy.iter().enumerate() {
        let b = k as u32 + 1;
        s.inputs.extend(inputs.iter().filter(|i| i.beat == b).copied());
        let (sim, rep, _) = Sim::advance(2, cfg, &mut s, b, *e);
        assert!(sim.list.len() <= 200, "beat {b}: {} cells", sim.list.len());
        assert!(sim.phen.len() <= 8);
        if !rep.evicted.is_empty() {
            // The calmest went: nothing evicted outweighs what was kept.
            assert!(sim.list.len() <= 200 * 15 / 16 + 1);
            assert!(rep.evicted.iter().all(|&i| sim.c(i).is_base()));
        }
        evicted += rep.evicted.len();
        refused += rep.refused;
    }
    assert!(evicted > 0 && s.evicted as usize == evicted);
    assert!(refused > 0, "a full list refuses the weak");
    // Capped runs are as exact as any: keyframes agree with the start.
    let (s2, h2) = play(2, cfg, &inputs, &entropy);
    assert_eq!(s2, s);
    assert_eq!(Sim::at_beat(2, cfg, &s, 23).unwrap().hash(), h2[22]);
}

// ---------------------------------------------------------------------------------------------------------------------
// What clients get
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn deltas_rebuild_the_field_and_fit_the_budget() {
    let cfg = Cfg::default();
    let (inputs, entropy) = heavy_inputs(60);
    let mut s = EarthState::default();
    // A client that starts with nothing and applies every beat's delta holds the server's field: heights and top
    // materials exactly, every layer to within a sixteenth of its range.
    let mut client: std::collections::BTreeMap<u16, (u8, u8, [u8; 14])> = Default::default();
    let (mut worst, mut sum) = (0usize, 0usize);
    let blank = Sim::new(13, cfg);
    for (k, e) in entropy.iter().enumerate() {
        let b = k as u32 + 1;
        s.inputs.extend(inputs.iter().filter(|i| i.beat == b).copied());
        let (sim, report, before) = Sim::advance(13, cfg, &mut s, b, *e);
        let eb = super::rules::EarthBeat { before, sim, report, entries: vec![], damaged: 0, micros: 0 };
        let v = view::beat_json(&eb);
        let bytes = serde_json::to_string(&v).unwrap().len();
        worst = worst.max(bytes);
        sum += bytes;
        let d = &v["delta"];
        assert_eq!(d["enc"], "pack");
        let cells = view::unpack_cells(&codec::un64(d["data"].as_str().unwrap()));
        assert_eq!(cells.len() as u64, d["n"].as_u64().unwrap());
        assert!(cells.windows(2).all(|w| w[0].0 < w[1].0), "in index order");
        for (i, h, top, layers) in cells {
            client.insert(i, (h, top, layers));
        }
        let sim = &eb.sim;
        let near = |a: u8, b: u8| a >> 4 == b >> 4;
        for i in 0..grid().len() as u16 {
            if sim.c(i).is_base() && !client.contains_key(&i) {
                continue;
            }
            let (h, top, layers) = client.get(&i).copied().unwrap_or_else(|| (grid().h[i as usize], blank.top(i), [0; 14]));
            assert_eq!((h, top), (sim.height(i), sim.top(i)), "beat {b} cell {i}: height and top are exact");
            let c = sim.c(i);
            for (name, mine, theirs) in [("water", layers[0], c.water), ("lava", layers[1], c.lava), ("fire", layers[3], c.fire),
                                         ("cloud", layers[5], c.cloud), ("ruin", layers[9], c.ruin), ("shake", layers[12], c.shake)] {
                assert!(near(mine, theirs), "beat {b} cell {i} {name}: the client has {mine}, the server {theirs}");
            }
        }
        assert!(v["phenomena"].as_array().unwrap().len() == sim.phen.len());
        // The three encodings say the same.
        if b == 30 {
            let cells = view::changed(&eb.before_sim(), sim);
            let (p, c, j) = (view::columns(sim, &cells, view::Enc::Pack), view::columns(sim, &cells, view::Enc::Columns), view::columns(sim, &cells, view::Enc::Json));
            let un = view::unpack_cells(&codec::un64(p["data"].as_str().unwrap()));
            assert_eq!(un.iter().map(|x| x.0 as u64).collect::<Vec<_>>(), j["cells"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect::<Vec<_>>());
            assert_eq!(un.iter().map(|x| x.1).collect::<Vec<_>>(), codec::un64(c["h"].as_str().unwrap()));
            assert_eq!(un.iter().map(|x| x.3[3]).collect::<Vec<_>>(), codec::un64(c["layers"]["fire"].as_str().unwrap()));
            assert_eq!(un.iter().map(|x| x.3[7] as i8 as i64).collect::<Vec<_>>(), j["layers"]["heat"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect::<Vec<_>>());
            let len = |v: &serde_json::Value| serde_json::to_string(v).unwrap().len();
            assert!(len(&p) < len(&c) && len(&c) < len(&j), "pack {} < columns {} < json {}", len(&p), len(&c), len(&j));
        }
    }
    eprintln!("heavy match: earth payload mean {} bytes, worst {worst}", sum / 60);
    assert!(worst < 64 * 1024, "a catastrophic beat's payload is {worst} bytes");
}

#[test]
fn finer_levels_are_a_pure_function_of_the_coarse_cell() {
    use fn7_model::hexgrid::{cell_at, HexLevel};
    let (sim, _) = run(14, &[spawn_at(WILDFIRE, AMAZON, 9, TIER_MEGA), spawn_at(FLOOD, [-58.2, -3.4], 4, TIER_MINI)], 2, 0);
    let coarse = cell(AMAZON);
    let fire = sim.c(coarse).fire;
    assert!(fire > 60);
    // The share of village hexes burning is the coarse intensity; the same hexes every time.
    let c0 = cell_at(AMAZON, HexLevel::Villages);
    let hexes: Vec<_> = (0..=12).flat_map(|k| c0.ring(k)).collect();
    let views: Vec<_> = hexes.iter().map(|c| view::hex_json(&sim, c)).collect();
    let burning = views.iter().filter(|v| v["fire"].as_u64().unwrap() > 0).count();
    let share = burning as f64 / hexes.len() as f64;
    // (Each hex follows the coarse cell under it; near a coarse cell's edge some follow its neighbour.)
    let expect = views.iter().map(|v| sim.c(v["coarse_index"].as_u64().unwrap() as u16).fire as f64 / 255.0).sum::<f64>() / hexes.len() as f64;
    assert!((share - expect).abs() < 0.08, "{share} of the hexes burn; the coarse cells say {expect}");
    assert_eq!(view::hex_json(&sim, &hexes[5]), view::hex_json(&sim, &hexes[5]));
    let v = view::hex_json(&sim, &hexes[0]);
    assert_eq!((v["derived"].as_bool(), v["coarse"].as_str()), (Some(true), Some(grid().key(coarse).as_str())));
    // A mini is drawn where it was cast: its own hex is flooded for certain; the regions level is the truth itself.
    let at = cell_at([-58.2, -3.4], HexLevel::Villages);
    assert!(view::hex_json(&sim, &at)["water"].as_u64().unwrap() > 0);
    let r = view::hex_json(&sim, &grid().cells[coarse as usize]);
    assert_eq!((r["derived"].as_bool(), r["fire"].as_u64()), (Some(false), Some(fire as u64)));
    // People level too.
    let p = view::hex_json(&sim, &cell_at(AMAZON, HexLevel::People));
    assert_eq!(p["level"], "people");
    assert!(p["h16"].as_u64().unwrap() >= 128);
}

#[test]
fn the_ascii_map_shows_it() {
    let (sim, _) = run(15, &[spawn_at(VOLCANO, HONSHU, 10, TIER_MEGA), spawn_at(WILDFIRE, AMAZON, 9, TIER_MEGA)], 3, 0);
    let map = view::ascii(&sim, [-90.0, -30.0, -30.0, 15.0], 60, 24, false);
    assert_eq!(map.lines().count(), 24);
    assert!(map.contains('&') && map.contains('"') && map.contains('~'), "{map}");
    let map = view::ascii(&sim, [125.0, 25.0, 150.0, 46.0], 50, 20, true);
    assert!(map.contains('V') && map.contains("\x1b["));
}

// ---------------------------------------------------------------------------------------------------------------------
// Sizes and speed (release build: `cargo test --release -p fn7-plugin earth -- --ignored --nocapture`)
// ---------------------------------------------------------------------------------------------------------------------

#[test]
#[ignore]
fn measure_sizes_and_speed() {
    let cfg = Cfg::default();
    for (name, inputs, entropy) in [
        ("calm (4 minis and a mega, Entropy 0–12)", {
            let mut v: Vec<EarthInput> = (1..=4).map(|b| input_at(b * 10, [WILDFIRE, STORM, FLOOD, LIGHTNING][b as usize % 4], [AMAZON, CONGO, BENGAL, HONSHU][b as usize % 4], 4, TIER_MINI)).collect();
            v.push(input_at(20, STORM, CONGO, 6, TIER_MEGA));
            v
        }, (1..=60u32).map(|b| (b / 5) as u8).collect::<Vec<u8>>()),
        ("busy (a mega and a mini a beat, Entropy to 60)", heavy_inputs(60).0.into_iter().filter(|i| i.owner == 0 || i.tier == TIER_MINI).collect(),
         (1..=60u32).map(|b| b as u8).collect()),
        ("heavy (4 megas a beat, Entropy to 100)", heavy_inputs(60).0, heavy_inputs(60).1),
    ] {
        let mut s = EarthState::default();
        let (mut step_us, mut worst_us, mut payload, mut worst_payload, mut state_bytes, mut worst_state) = (0u128, 0u128, 0usize, 0usize, 0usize, 0usize);
        let (mut cells, mut worst_cells, mut phen) = (0usize, 0usize, 0usize);
        let mut last_us = 0;
        let (mut encs, mut sent, mut all) = ([0usize; 3], 0usize, 0usize);
        for (k, e) in entropy.iter().enumerate() {
            let b = k as u32 + 1;
            s.inputs.extend(inputs.iter().filter(|i| i.beat == b).copied());
            let t = std::time::Instant::now();
            let (sim, report, before) = Sim::advance(13, cfg, &mut s, b, *e);
            let us = t.elapsed().as_micros();
            step_us += us;
            worst_us = worst_us.max(us);
            last_us = us;
            let eb = super::rules::EarthBeat { before, sim, report, entries: vec![], damaged: 0, micros: 0 };
            let bytes = serde_json::to_string(&view::beat_json(&eb)).unwrap().len();
            payload += bytes;
            worst_payload = worst_payload.max(bytes);
            let ch = view::changed(&eb.before_sim(), &eb.sim);
            let every = eb.sim.list.len().max(eb.before.len());
            for (k, enc) in [view::Enc::Pack, view::Enc::Columns, view::Enc::Json].into_iter().enumerate() {
                encs[k] += serde_json::to_string(&view::columns(&eb.sim, &ch, enc)).unwrap().len();
            }
            sent += ch.len();
            all += every;
            let sb = serde_json::to_string(&s).unwrap().len();
            state_bytes += sb;
            worst_state = worst_state.max(sb);
            cells += eb.sim.list.len();
            worst_cells = worst_cells.max(eb.sim.list.len());
            phen = phen.max(eb.sim.phen.len());
        }
        let kf: Vec<usize> = s.keyframes.iter().map(|k| serde_json::to_string(k).unwrap().len()).collect();
        let t = std::time::Instant::now();
        let from_kf = Sim::at_beat(13, cfg, &s, 55).unwrap().hash();
        let kf_us = t.elapsed().as_micros();
        let t = std::time::Instant::now();
        let from_start = Sim::at_beat_from(13, cfg, &s, 55, false).unwrap().hash();
        let start_us = t.elapsed().as_micros();
        assert_eq!(from_kf, from_start);
        eprintln!("{name}: delta alone per beat: pack {} bytes, columns {}, json {}; cells sent {} of {} dirty per beat", encs[0] / 60, encs[1] / 60, encs[2] / 60, sent / 60, all / 60);
        eprintln!("{name}:\n  step (load + step + store + keyframe): mean {} us, worst {} us, the last beat {} us\n  dirty cells: mean {}, worst {}; phenomena at most {}\n  earth payload per beat: mean {} bytes, worst {}\n  state (dm.earth as JSON): mean {} bytes per snapshot, worst {}, at the end {} (inputs {} = {} bytes, cells {} bytes)\n  keyframes: {} of them, bytes {:?}, total {}\n  beat 55 from its keyframe: {} us; from beat 1: {} us",
                  step_us / 60, worst_us, last_us, cells / 60, worst_cells, phen, payload / 60, worst_payload, state_bytes / 60, worst_state,
                  serde_json::to_string(&s).unwrap().len(), s.inputs.len(), serde_json::to_string(&s.inputs).unwrap().len(), s.cells.len(),
                  kf.len(), kf, kf.iter().sum::<usize>(), kf_us, start_us);
    }
}

#[test]
#[ignore]
fn look_at_the_world() {
    let sim = Sim::new(7, Cfg::default());
    eprintln!("{}", view::ascii(&sim, [-180.0, -80.0, 180.0, 85.0], 180, 56, false));
}

