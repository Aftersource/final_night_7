//! People who emerge from the simulation (ringleaders, generals, captains, heroes, cowards, witnesses, prophets,
//! paladins), their sheets, their deaths, and the petty goals they pursue by rules every beat.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde_json::json;

use crate::geo::{city_of, distance_km, pos_of};
use crate::sim::{clamp01, r3, Ctx};

/// How a new person should come out: their role, rank and leanings.
pub struct Spec {
    pub role: Role,
    pub level: Level,
    pub of: Option<Ref>,
    /// "General of …", "ringleader of …".
    pub title: String,
    /// How the world will call them ("the ringleader of the bread mob").
    pub alias: String,
    /// Leanings added to the seeded rolls (-1..1 each).
    pub competence: f32,
    pub courage: f32,
    pub ambition: f32,
    pub cruelty: f32,
}

impl Spec {
    pub fn new(role: Role, level: Level, of: Option<Ref>, title: String, alias: String) -> Self {
        Spec {
            role,
            level,
            of,
            title,
            alias,
            competence: 0.0,
            courage: 0.0,
            ambition: 0.0,
            cruelty: 0.0,
        }
    }
}

/// The surname of a "Given Surname" name.
pub fn surname(name: &str) -> &str {
    name.rsplit(' ').next().unwrap_or(name)
}

/// Bring the next person of crowd (or army) `from` into the story with a sheet. `structural` people (a new army's
/// general and captains) may go past the per-beat emergence budget. Emits `person_emerged`.
pub fn emerge(w: &mut World, cx: &mut Ctx, from: ClusterId, spec: Spec, structural: bool) -> Option<CharId> {
    if !structural && !cx.can_emerge() {
        return None;
    }
    let cl = w.clusters.get(from.ix())?;
    if cl.size < 2 {
        return None;
    }
    let index = cl.materialized;
    let seed = cl.seed;
    let army_pos = cl.army.as_ref().map(|a| a.pos);
    let m = Mention {
        text: spec.alias.clone(),
        kind: "person".into(),
        near: None,
    };
    let id = crate::mention::materialize(w, from, &m, cx.beat, None);
    cx.emerge_left = cx.emerge_left.saturating_sub(1);
    w.soc.people_emerged += 1;
    let mut rng = Rng::derive(seed, &[index as u64, hash_str("life")]);
    let mut roll = |bias: f32| clamp01(0.15 + 0.7 * rng.f64() as f32 + bias * 0.35);
    let competence = roll(spec.competence);
    let courage = roll(spec.courage);
    let ambition = roll(spec.ambition);
    let cruelty = r3(((rng.f64() as f32) * 2.0 - 1.0 + spec.cruelty * 0.5).clamp(-1.0, 1.0));
    let crowd = &w.clusters[from.ix()];
    let need = crowd.need;
    let goal = match spec.role {
        Role::General | Role::Captain | Role::Soldier | Role::Paladin => Goal::Serve,
        Role::Prophet => Goal::Preach,
        Role::Ringleader if need.food > 0.5 => Goal::Loot,
        Role::Coward => Goal::Flee,
        _ if ambition > 0.7 => Goal::Promotion,
        _ => Goal::Survive,
    };
    let ch = &mut w.characters[id.ix()];
    if let Some(p) = army_pos {
        ch.pos = p;
    }
    let sur = surname(&ch.name).to_string();
    let title_alias = match spec.role {
        Role::General => Some(format!("General {sur}")),
        Role::Captain => Some(format!("Captain {sur}")),
        Role::Prophet => Some(format!("the Prophet {sur}")),
        _ => None,
    };
    if let Some(a) = title_alias {
        ch.aliases.push(a);
    }
    if !spec.title.is_empty() && !ch.aliases.contains(&spec.title) {
        ch.aliases.push(spec.title.clone());
    }
    ch.life = Some(Box::new(Life {
        role: spec.role,
        rank: Rank {
            level: spec.level,
            of: spec.of,
            title: spec.title.clone(),
        },
        competence,
        ambition,
        cruelty,
        courage,
        grudges: vec![],
        need,
        goal,
        emerged_as: spec.role,
        emerged: cx.beat,
        deeds: 0,
        kills: 0,
        cell: None,
        army: None,
        mob: None,
        died: None,
    }));
    let name = ch.name.clone();
    let at = ch.pos;
    let place = ch.place;
    let label = crowd.label.clone();
    let role = format!("{:?}", spec.role).to_lowercase();
    cx.emit(
        w,
        SocKind::PersonEmerged,
        format!("{name} steps out of the {label} as {}", spec.title),
        Ref::Char(id),
        vec![Ref::Char(id), Ref::Cluster(from)],
        at,
        Some(place),
        3,
        0,
        json!({ "person": Ref::Char(id), "name": name, "role": role, "title": spec.title, "from": Ref::Cluster(from),
                "competence": competence, "courage": courage, "ambition": ambition, "cruelty": cruelty }),
    );
    Some(id)
}

pub fn life(w: &World, c: CharId) -> Option<&Life> {
    w.characters.get(c.ix()).and_then(|x| x.life.as_deref())
}

pub fn life_mut(w: &mut World, c: CharId) -> Option<&mut Life> {
    w.characters.get_mut(c.ix()).and_then(|x| x.life.as_deref_mut())
}

pub fn alive(w: &World, c: Option<CharId>) -> Option<CharId> {
    c.filter(|c| w.characters.get(c.ix()).is_some_and(|x| x.alive))
}

/// A free veteran of `city` (alive, holding one of `roles`, leading no mob and serving in no army, not a god's, not
/// possessed, not hostile to `side`): the most storied first (deeds, then competence). Persistent people come back
/// instead of new ones stepping out every time.
pub fn veteran(w: &World, city: PlaceId, roles: &[Role], side: Option<DemigodId>) -> Option<CharId> {
    w.characters
        .iter()
        .filter(|c| c.alive && c.serves.is_none() && c.possessed_by.is_none())
        .filter(|c| c.life.as_deref().is_some_and(|l| roles.contains(&l.role) && l.mob.is_none() && l.army.is_none()))
        .filter(|c| city_of(w, c.place) == Some(city))
        .filter(|c| side.is_none_or(|d| crate::util::get(&c.allegiance, d) >= -0.1))
        .max_by(|x, y| {
            let s = |c: &Character| c.life.as_deref().map(|l| (l.deeds, l.competence)).unwrap_or((0, 0.0));
            let (a, b) = (s(x), s(y));
            a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(y.id.cmp(&x.id))
        })
        .map(|c| c.id)
}

/// Why `me` holds a grudge against `target`, as a phrase ("for the riot their soldiers broke"), from what the two have
/// been to each other.
pub fn grudge_reason(w: &World, me: CharId, target: Ref) -> String {
    let l = life(w, me);
    let my_role = l.map(|l| l.role).unwrap_or_default();
    let was_ringleader = l.is_some_and(|l| l.emerged_as == Role::Ringleader);
    match target {
        Ref::Char(t) => {
            let tl = life(w, t);
            let t_role = tl.map(|l| l.role).unwrap_or_default();
            let same_army = l.and_then(|l| l.army).is_some() && l.and_then(|l| l.army) == tl.and_then(|l| l.army);
            match (my_role, t_role) {
                (Role::Captain, Role::General) if same_army => "for the command they stole".into(),
                (Role::Coward, Role::General) => "for the shame of the rout they were blamed for".into(),
                (Role::Deserter, Role::General) => "for the treason they would not follow".into(),
                (Role::General, _) if same_army => "for the coup that cost them the army".into(),
                (_, Role::General) if was_ringleader => "for the riot their soldiers broke".into(),
                (Role::Ringleader, Role::Ringleader) | (Role::Prophet, _) | (_, Role::Prophet) => {
                    "for the street fight between their people".into()
                }
                (_, Role::Avenger) => "for the blood they spilled".into(),
                _ => "for an old wrong between their families".into(),
            }
        }
        Ref::Demigod(d) => {
            let dn = w.demigods.get(d.ix()).map(|x| x.name.clone()).unwrap_or_default();
            match w.characters.get(me.ix()).and_then(|c| c.serves) {
                Some(g) => format!("in the name of {}, who has turned against {dn}", w.gods.get(g.ix()).map(|x| x.name.as_str()).unwrap_or("a god")),
                None => format!("for what {dn}'s war did to their home"),
            }
        }
        _ => "over an old score".into(),
    }
}

/// Give someone a role (an addressed person promoted, a soldier made captain).
pub fn set_role(w: &mut World, c: CharId, role: Role, level: Level, of: Option<Ref>, title: String, beat: u32) {
    let Some(ch) = w.characters.get_mut(c.ix()) else { return };
    let sur = surname(&ch.name).to_string();
    let alias = match role {
        Role::General => Some(format!("General {sur}")),
        Role::Captain => Some(format!("Captain {sur}")),
        _ => None,
    };
    if let Some(a) = alias {
        if !ch.aliases.contains(&a) {
            ch.aliases.push(a);
        }
    }
    if !title.is_empty() && !ch.aliases.contains(&title) {
        ch.aliases.push(title.clone());
    }
    match ch.life.as_deref_mut() {
        Some(l) => {
            l.role = role;
            l.rank = Rank { level, of, title };
            if matches!(role, Role::General | Role::Captain) {
                l.goal = Goal::Serve;
            }
        }
        None => {
            let mut rng = Rng::derive(w.meta.seed, &[hash_str("role"), c.0 as u64]);
            let mut roll = || clamp01(0.3 + 0.6 * rng.f64() as f32);
            ch.life = Some(Box::new(Life {
                role,
                rank: Rank { level, of, title },
                competence: roll(),
                ambition: roll(),
                cruelty: 0.0,
                courage: roll(),
                grudges: vec![],
                need: Need::default(),
                goal: Goal::Serve,
                emerged_as: role,
                emerged: beat,
                deeds: 0,
                kills: 0,
                cell: None,
                army: None,
                mob: None,
                died: None,
            }));
        }
    }
}

/// A person dies (in battle, by a mob, by revenge). Emits `death` for anyone with a role (soldiers only in focus).
pub fn kill(w: &mut World, cx: &mut Ctx, c: CharId, cause: &str, by: Option<Ref>) {
    let Some(ch) = w.characters.get_mut(c.ix()) else { return };
    if !ch.alive {
        return;
    }
    ch.alive = false;
    ch.health = 0.0;
    let role = ch.life.as_ref().map(|l| l.role).unwrap_or_default();
    if let Some(l) = ch.life.as_deref_mut() {
        l.died = Some(cx.beat);
    }
    let (name, at, place) = (ch.name.clone(), ch.pos, ch.place);
    crate::util::end_agents_for(w, Ref::Char(c), cx.beat);
    let title = life(w, c).map(|l| l.rank.title.clone()).unwrap_or_default();
    let who = if title.is_empty() { name.clone() } else { format!("{name}, {title},") };
    let mag = match role {
        Role::General => 7,
        Role::Ringleader | Role::Prophet | Role::Hero => 6,
        Role::Captain | Role::Notable | Role::Paladin => 5,
        _ => 3,
    };
    let city = city_of(w, place).unwrap_or(place);
    let mut refs = vec![Ref::Char(c)];
    refs.extend(by);
    cx.emit(
        w,
        SocKind::Death,
        format!("{who} dies: {cause}"),
        Ref::Place(city),
        refs,
        at,
        Some(place),
        mag,
        -3,
        json!({ "person": Ref::Char(c), "name": name, "role": format!("{role:?}").to_lowercase(), "cause": cause, "by": by }),
    );
}

/// Add a grudge (deduplicated, at most 4, newest kept).
pub fn grudge(w: &mut World, c: CharId, against: Ref) {
    if against == Ref::Char(c) {
        return;
    }
    if let Some(l) = life_mut(w, c) {
        if !l.grudges.contains(&against) {
            l.grudges.push(against);
            if l.grudges.len() > 4 {
                l.grudges.remove(0);
            }
        }
        if l.cruelty > -0.2 || l.ambition > 0.6 {
            l.goal = Goal::Revenge(against);
        }
    }
}

/// The hand's `Promote`: raise `who` to general or captain of an army of `by` (its biggest when `army` is None), or
/// make them the ringleader/prophet where they stand. A replaced general is demoted to captain and holds a grudge.
pub fn promote(
    w: &mut World,
    who: CharId,
    by: DemigodId,
    role: Role,
    army: Option<ClusterId>,
    beat: u32,
) -> Result<Vec<Ref>, String> {
    let ch = w.characters.get(who.ix()).ok_or("no such person")?;
    if !ch.alive {
        return Err(format!("{who} is dead"));
    }
    if ch.serves.is_some() {
        return Err("a god's paladin can't be promoted by a demigod".into());
    }
    let mut refs = vec![Ref::Char(who)];
    match role {
        Role::General | Role::Captain => {
            let army = match army {
                Some(a) => a,
                None => crate::world_rules::armies(w)
                    .filter(|(_, a)| a.demigod == by && !a.rogue)
                    .max_by(|x, y| x.0.size.cmp(&y.0.size).then(y.0.id.cmp(&x.0.id)))
                    .map(|(c, _)| c.id)
                    .ok_or("no army to promote them into")?,
            };
            let cl = w.clusters.get(army.ix()).ok_or("no such army")?;
            let a = cl.army.as_ref().ok_or("not an army")?;
            if !w.same_player(a.demigod, by) {
                return Err("not your army".into());
            }
            let label = cl.label.clone();
            let pos = a.pos;
            refs.push(Ref::Cluster(army));
            if role == Role::General {
                let old = a.general.filter(|g| *g != who);
                set_role(w, who, Role::General, Level::Crowd, Some(Ref::Cluster(army)), format!("General of {label}"), beat);
                if let Some(o) = old {
                    set_role(w, o, Role::Captain, Level::Cell, Some(Ref::Cluster(army)), format!("captain in {label}"), beat);
                    grudge(w, o, Ref::Char(who));
                    refs.push(Ref::Char(o));
                }
                let a = w.clusters[army.ix()].army.as_mut().expect("army");
                a.captains.retain(|c| *c != who);
                if let Some(o) = old {
                    if !a.captains.contains(&o) {
                        a.captains.push(o);
                    }
                }
                a.general = Some(who);
            } else {
                set_role(w, who, Role::Captain, Level::Cell, Some(Ref::Cluster(army)), format!("captain in {label}"), beat);
                let a = w.clusters[army.ix()].army.as_mut().expect("army");
                if !a.captains.contains(&who) && a.general != Some(who) {
                    a.captains.push(who);
                }
            }
            if let Some(l) = life_mut(w, who) {
                l.army = Some(army);
                l.deeds += 1;
            }
            let ch = &mut w.characters[who.ix()];
            ch.pos = pos;
            crate::util::bump(&mut ch.allegiance, by, 0.3, -1.0, 1.0);
        }
        Role::Ringleader | Role::Prophet | Role::Hero | Role::Notable => {
            let place = ch.place;
            let city = city_of(w, place).unwrap_or(place);
            let title = format!("{:?} of {}", role, w.name_of(Ref::Place(city)));
            set_role(w, who, role, Level::City, Some(Ref::Place(city)), title, beat);
            let ch = &mut w.characters[who.ix()];
            crate::util::bump(&mut ch.allegiance, by, 0.25, -1.0, 1.0);
            refs.push(Ref::Place(city));
        }
        _ => return Err(format!("can't promote to {role:?}")),
    }
    w.soc.promotions += 1;
    Ok(refs)
}

/// Every beat, every living person with a sheet: needs follow their crowd's or army's; goals are re-chosen; ambition,
/// grudges and fear act (bounded: at most a few acts per beat, all by seeded rolls).
pub fn step(w: &mut World, cx: &mut Ctx) {
    let ids: Vec<CharId> = w
        .characters
        .iter()
        .filter(|c| c.alive && c.life.is_some() && c.serves.is_none_or(|_| c.life.as_ref().is_some_and(|l| l.role == Role::Paladin)))
        .map(|c| c.id)
        .collect();
    let mut acts = 0;
    for id in ids {
        if !w.characters[id.ix()].alive {
            continue;
        }
        // Needs: the army's or the home crowd's.
        let (army, home) = {
            let l = life(w, id).expect("has life");
            let home = match w.characters[id.ix()].origin {
                Origin::Materialized { cluster, .. } => Some(cluster),
                _ => None,
            };
            (l.army, home)
        };
        let need = match army.and_then(|a| w.clusters.get(a.ix())).and_then(|c| c.army.as_ref()) {
            Some(a) => Need {
                food: clamp01(1.0 - a.supply),
                safety: clamp01(a.strain),
                faith: 0.2,
            },
            None => home.and_then(|h| w.clusters.get(h.ix())).map(|c| c.need).unwrap_or_default(),
        };
        let l = life_mut(w, id).expect("has life");
        l.need = need;
        // Goal: revenge first (a living target), fear, ambition, role.
        let target_alive = |w: &World, r: Ref| crate::util::is_alive(w, r);
        let grudges = l.grudges.clone();
        let (courage, ambition, role) = (l.courage, l.ambition, l.role);
        let revenge = grudges.into_iter().rev().find(|g| target_alive(w, *g));
        let goal = if let Some(g) = revenge.filter(|_| ambition > 0.35 || courage > 0.5) {
            Goal::Revenge(g)
        } else if courage < 0.3 && need.safety > 0.6 && role != Role::General {
            Goal::Flee
        } else if role == Role::Prophet {
            Goal::Preach
        } else if role == Role::Captain && ambition > 0.65 {
            Goal::Promotion
        } else if matches!(role, Role::General | Role::Captain | Role::Soldier | Role::Paladin) {
            Goal::Serve
        } else if need.food > 0.6 {
            Goal::Loot
        } else if ambition > 0.75 {
            Goal::Promotion
        } else {
            Goal::Survive
        };
        life_mut(w, id).expect("has life").goal = goal;
        if acts >= 4 {
            continue;
        }
        let acted = match goal {
            Goal::Revenge(t) => revenge_attempt(w, cx, id, t),
            Goal::Flee => flee(w, cx, id),
            Goal::Promotion => seek_promotion(w, cx, id),
            _ => false,
        };
        if acted {
            acts += 1;
        }
    }
}

/// A grudge acted on: when the target is near (same city, same army, or a demigod within 150 km), a seeded chance of
/// an attempt; competence against the target's decides who dies. Hitting a demigod is a proposal (small damage).
fn revenge_attempt(w: &mut World, cx: &mut Ctx, id: CharId, target: Ref) -> bool {
    let me = &w.characters[id.ix()];
    let l = me.life.as_deref().expect("life");
    let (comp, cruelty, my_pos, my_place, name) = (l.competence, l.cruelty, me.pos, me.place, me.name.clone());
    let my_army = l.army;
    let Some(tpos) = pos_of(w, target) else { return false };
    let near = match target {
        Ref::Char(t) => {
            let tl = life(w, t);
            distance_km(my_pos, tpos) < 120.0 || (my_army.is_some() && tl.and_then(|x| x.army) == my_army)
        }
        Ref::Demigod(_) => distance_km(my_pos, tpos) < 150.0,
        _ => false,
    };
    if !near {
        return false;
    }
    let p = 0.06 + 0.08 * cruelty.max(0.0) + 0.04 * comp;
    if cx.roll("revenge", id.0 as u64) >= p {
        return false;
    }
    let tname = w.name_of(target);
    let why = grudge_reason(w, id, target);
    let cname = crate::sim::near_name(w, my_pos);
    match target {
        Ref::Demigod(d) => {
            let amount = r3(0.5 + comp);
            cx.out.proposals.push(Proposal {
                source: Source::Rules,
                intent: None,
                effect: Effect::Damage {
                    who: Ref::Demigod(d),
                    amount,
                    cause: format!("{name}'s revenge"),
                },
                act: format!("{name} slips through {tname}'s guard near {cname} with a blade, {why}"),
                actor: Ref::Char(id),
                magnitude: 6,
                valence: -3,
                witnesses: vec![Ref::Char(id), Ref::Demigod(d)],
                cause: None,
            });
            if let Some(l) = life_mut(w, id) {
                l.deeds += 1;
                l.grudges.retain(|g| *g != target);
            }
            cx.emit(
                w,
                SocKind::Revenge,
                format!("{name} slips through {tname}'s guard near {cname} and draws blood, {why}"),
                Ref::Char(id),
                vec![Ref::Char(id), target],
                my_pos,
                Some(my_place),
                6,
                -3,
                json!({ "avenger": Ref::Char(id), "target": target, "amount": amount, "why": why }),
            );
            true
        }
        Ref::Char(t) => {
            let their = life(w, t).map(|x| (x.competence + x.courage) / 2.0).unwrap_or(0.4);
            let win = cx.roll("revenge_win", id.0 as u64) < 0.35 + comp - 0.5 * their;
            cx.emit(
                w,
                SocKind::Revenge,
                if win {
                    format!("{name} ambushes {tname} in {cname} and kills them, {why}")
                } else {
                    format!("{name} goes for {tname} with a knife in {cname}, {why}, and is cut down by {tname}'s people")
                },
                Ref::Char(id),
                vec![Ref::Char(id), target],
                my_pos,
                Some(my_place),
                6,
                -3,
                json!({ "avenger": Ref::Char(id), "target": target, "won": win, "why": why }),
            );
            if win {
                kill(w, cx, t, &format!("ambushed by {name} in {cname}, {why}"), Some(Ref::Char(id)));
                if let Some(l) = life_mut(w, id) {
                    l.kills += 1;
                    l.deeds += 1;
                    l.grudges.retain(|g| *g != target);
                    l.role = Role::Avenger;
                }
            } else {
                kill(w, cx, id, &format!("cut down by {tname}'s people while trying to kill them"), Some(target));
            }
            true
        }
        _ => false,
    }
}

/// Fear acted on: someone with little courage in danger slips away from their army or mob to the safest nearby city.
fn flee(w: &mut World, cx: &mut Ctx, id: CharId) -> bool {
    if cx.roll("flee", id.0 as u64) >= 0.15 {
        return false;
    }
    let l = life(w, id).expect("life").clone();
    if l.army.is_none() && l.mob.is_none() {
        return false;
    }
    let me = &w.characters[id.ix()];
    let (name, pos) = (me.name.clone(), me.pos);
    let from = l
        .army
        .map(Ref::Cluster)
        .or(l.mob.map(Ref::Mob))
        .expect("checked");
    let fname = w.name_of(from);
    let safest = w
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::Site || (!w.is_planet() && p.kind == PlaceKind::District))
        .filter(|p| distance_km(p.center, pos) < 1500.0)
        .map(|p| (w.nodes.get(p.id.ix()).map(|n| n.need.safety).unwrap_or(0.5), p.id, p.center))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if let Some(a) = l.army {
        if let Some(ar) = w.clusters[a.ix()].army.as_mut() {
            ar.captains.retain(|c| *c != id);
            if ar.general == Some(id) {
                ar.general = None;
            }
        }
    }
    if let Some((_, p, c)) = safest {
        let ch = &mut w.characters[id.ix()];
        ch.place = p;
        ch.pos = c;
    }
    if let Some(l) = life_mut(w, id) {
        l.role = Role::Deserter;
        l.army = None;
        l.mob = None;
        l.goal = Goal::Survive;
        l.rank = Rank {
            level: Level::Person,
            of: None,
            title: format!("deserter from {fname}"),
        };
    }
    w.soc.deserters += 1;
    cx.emit(
        w,
        SocKind::Desertion,
        format!("{name} loses their nerve and slips away from {fname}"),
        Ref::Char(id),
        vec![Ref::Char(id), from],
        pos,
        Some(w.characters[id.ix()].place),
        4,
        -1,
        json!({ "person": Ref::Char(id), "from": from }),
    );
    true
}

/// Ambition acted on: a captain who outclasses a struggling general (strain high) moves against them; a ringleader
/// with nothing left to lead offers themselves to the demigod they love (a volunteer host, see `armies`).
fn seek_promotion(w: &mut World, cx: &mut Ctx, id: CharId) -> bool {
    let l = life(w, id).expect("life").clone();
    let Some(army) = l.army else { return false };
    let Some(a) = w.clusters.get(army.ix()).and_then(|c| c.army.clone()) else {
        return false;
    };
    let Some(g) = alive(w, a.general) else { return false };
    if g == id {
        return false;
    }
    let gl = life(w, g).cloned();
    let gcomp = gl.as_ref().map(|x| x.competence).unwrap_or(0.5);
    if a.strain < 0.45 || l.competence <= gcomp {
        return false;
    }
    if cx.roll("coup", id.0 as u64) >= 0.12 + 0.2 * (l.ambition - 0.65).max(0.0) {
        return false;
    }
    let (name, gname) = (w.characters[id.ix()].name.clone(), w.characters[g.ix()].name.clone());
    let label = w.clusters[army.ix()].label.clone();
    let pos = a.pos;
    let place = w.clusters[army.ix()].place;
    // The coup: the old general dies or is cast down.
    let ruthless = l.cruelty > 0.2;
    cx.emit(
        w,
        SocKind::Mutiny,
        format!("Captain {name} turns {label} against General {gname}{}", if ruthless { " and has them killed" } else { " and takes command" }),
        Ref::Char(id),
        vec![Ref::Char(id), Ref::Char(g), Ref::Cluster(army)],
        pos,
        Some(place),
        7,
        -2,
        json!({ "army": Ref::Cluster(army), "by": Ref::Char(id), "against": Ref::Char(g), "kind": "coup" }),
    );
    w.soc.mutinies += 1;
    if ruthless {
        kill(w, cx, g, &format!("killed in Captain {name}'s coup"), Some(Ref::Char(id)));
    } else {
        set_role(w, g, Role::Captain, Level::Cell, Some(Ref::Cluster(army)), format!("captain in {label}"), cx.beat);
        grudge(w, g, Ref::Char(id));
    }
    set_role(w, id, Role::General, Level::Crowd, Some(Ref::Cluster(army)), format!("General of {label}"), cx.beat);
    let ar = w.clusters[army.ix()].army.as_mut().expect("army");
    ar.general = Some(id);
    ar.captains.retain(|c| *c != id);
    if !ruthless && !ar.captains.contains(&g) {
        ar.captains.push(g);
    }
    ar.strain = r3(ar.strain * 0.6);
    w.soc.promotions += 1;
    cx.emit(
        w,
        SocKind::Promotion,
        format!("{name} is General of {label} now"),
        Ref::Char(id),
        vec![Ref::Char(id), Ref::Cluster(army)],
        pos,
        Some(place),
        5,
        0,
        json!({ "person": Ref::Char(id), "to": "general", "army": Ref::Cluster(army), "how": "coup" }),
    );
    true
}
