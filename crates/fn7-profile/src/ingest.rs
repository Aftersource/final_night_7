//! From a finished (or abandoned) match to profile rows.
//!
//! Two ways in, one shape ([`MatchInput`]):
//! - **Live**: at match end the server builds a `MatchInput` from the room's seats (display names) and the
//!   `match.ended` summary ([`MatchInput::from_match_ended`]) and calls [`crate::ProfileStore::record_match`].
//! - **Backfill**: [`read_recording`] streams a core JSONL recording (`<THEATRE_RECORD_DIR>/<match_id>.jsonl`) and
//!   keeps only what it needs: `run_start` (seats, room code, settings), the first `ruleset` (rules version), the
//!   last `round.committed` public state (demigod identities and portraits), every `elimination` event, and `run_end`
//!   (the summary). Lines are classified by their first bytes, so the multi-megabyte snapshot, hook and frame lines
//!   are never parsed. A recording with no `run_end` is an **unfinished** match (the server stopped, or it is still
//!   running): it is recorded as such and upgraded if it is ingested again after it ends.
//!
//! What a player's row holds comes only from what every seat saw at match end (the public summary and public state):
//! no seat's private view, no `player_id`, no resume token.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::{Deed, Elimination, MatchDemigod, NotableItem};

/// Where a match record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Live,
    Backfill,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Live => "live",
            Source::Backfill => "backfill",
        }
    }
}

/// One seat as the room had it (`run_start.meta.seats[]` / the core's `SeatInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct SeatIn {
    pub seat: u8,
    pub display_name: String,
    pub is_bot: bool,
}

/// Everything the store needs about one match.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchInput {
    pub match_id: String,
    pub room_code: Option<String>,
    /// Epoch ms.
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub rules_version: Option<String>,
    pub beats_total: Option<u32>,
    pub forked_from: Option<String>,
    pub seats: Vec<SeatIn>,
    /// The plugin's `match.ended` summary object (`summary.summary` in a `run_end` record). `None` = unfinished.
    pub summary: Option<Value>,
    /// Rounds played (`run_end.summary.rounds`, else the last committed beat).
    pub rounds: Option<u32>,
    /// The last `round.committed` public state (demigods' visual sheets and portrait/diorama images).
    pub public_state: Option<Value>,
    /// Every `elimination` event's data (`{beat, cause, chain, demigod, name}`).
    pub eliminations: Vec<Value>,
    /// The recording's file name, for time travel.
    pub recording: Option<String>,
    pub source: Source,
}

impl MatchInput {
    /// From the live path: the room's seats and a `match.ended` payload (its `d` object or the whole frame).
    pub fn from_match_ended(seats: Vec<SeatIn>, match_ended: &Value, ended_at: Option<i64>) -> Option<Self> {
        let d = match_ended.get("d").unwrap_or(match_ended);
        let match_id = d.get("match_id")?.as_str()?.to_string();
        let summary = d.get("summary").cloned();
        let rounds = d
            .get("rounds")
            .and_then(Value::as_u64)
            .map(|r| r as u32)
            .or_else(|| summary.as_ref().and_then(|s| s["stats"]["beats"].as_u64()).map(|r| r as u32));
        Some(Self {
            match_id,
            room_code: None,
            started_at: None,
            ended_at,
            rules_version: None,
            beats_total: summary.as_ref().and_then(|s| s["stats"]["beats_total"].as_u64()).map(|b| b as u32),
            forked_from: None,
            seats,
            summary,
            rounds,
            public_state: None,
            eliminations: vec![],
            recording: None,
            source: Source::Live,
        })
    }

    /// Finished (has a summary).
    pub fn ended(&self) -> bool {
        self.summary.is_some()
    }
}

/// What a recording scan saw, for the report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanStats {
    pub lines: u64,
    pub bytes: u64,
    pub parsed: u64,
}

/// A record of type `ty` (quoted): the core writes compact JSON with `type` first, `{"type":"run_start",...`.
fn starts(line: &str, ty: &str) -> bool {
    line.strip_prefix("{\"type\":").is_some_and(|rest| rest.starts_with(ty))
}

fn push_img(
    images: &mut Vec<ImageSeen>,
    img: &Value,
    kind: &str,
    caption: String,
    subject: Option<String>,
    subject_name: Option<String>,
) {
    if let Some(h) = hash_of(img) {
        if !images.iter().any(|i| i.hash == h) {
            let kind = img["kind"].as_str().unwrap_or(kind).to_string();
            images.push(ImageSeen { hash: h, kind, caption, subject, subject_name });
        }
    }
}

fn event_kind<'a>(line: &'a str, kinds: &[&'a str]) -> Option<&'a str> {
    if !line.starts_with("{\"type\":\"event\"") {
        return None;
    }
    // The kind follows the ctx object, well within the first 400 bytes.
    let mut cut = line.len().min(400);
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    let head = &line[..cut];
    let at = head.find("\"kind\":\"")? + 8;
    let rest = &head[at..];
    let end = rest.find('"')?;
    kinds.iter().copied().find(|k| *k == &rest[..end])
}

/// Stream a core JSONL recording into a [`MatchInput`] (`None` if it has no `run_start`, or isn't Final Night 7).
pub fn read_recording(path: &Path) -> anyhow::Result<(Option<MatchInput>, ScanStats)> {
    let f = std::fs::File::open(path)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    read_recording_from(BufReader::with_capacity(1 << 20, f), name)
}

/// [`read_recording`] over any reader (tests feed it a string).
pub fn read_recording_from<R: Read>(
    mut r: BufReader<R>,
    file_name: Option<String>,
) -> anyhow::Result<(Option<MatchInput>, ScanStats)> {
    let mut st = ScanStats::default();
    let mut line = String::new();
    let mut start: Option<Value> = None;
    let mut end: Option<Value> = None;
    let mut rules: Option<String> = None;
    let mut last_committed: Option<String> = None;
    let mut elims: Vec<Value> = vec![];
    loop {
        line.clear();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        st.lines += 1;
        st.bytes += n as u64;
        let l = line.trim_end();
        if starts(l, "\"run_start\"") {
            start = serde_json::from_str(l).ok();
            st.parsed += 1;
        } else if starts(l, "\"run_end\"") {
            end = serde_json::from_str(l).ok();
            st.parsed += 1;
        } else if rules.is_none() && starts(l, "\"ruleset\"") {
            st.parsed += 1;
            if let Ok(v) = serde_json::from_str::<Value>(l) {
                let label = v["label"].as_str().unwrap_or_default();
                if !label.is_empty() && label.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    rules = Some(label.to_string());
                }
            }
        } else if let Some(k) = event_kind(l, &["round.committed", "elimination"]) {
            if k == "round.committed" {
                // Keep the raw line; only the last one is parsed.
                last_committed = Some(l.to_string());
            } else if let Ok(v) = serde_json::from_str::<Value>(l) {
                st.parsed += 1;
                elims.push(v["data"].clone());
            }
        }
    }
    let Some(start) = start else {
        return Ok((None, st));
    };
    if start["plugin"].as_str().is_some_and(|p| p != "final_night_7") {
        return Ok((None, st));
    }
    let public_state = last_committed.and_then(|l| {
        st.parsed += 1;
        serde_json::from_str::<Value>(&l).ok().map(|v| v["data"]["public_state"].clone())
    });
    let meta = &start["meta"];
    let seats = meta["seats"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    Some(SeatIn {
                        seat: s["seat"].as_u64()? as u8,
                        display_name: s["display_name"].as_str().unwrap_or_default().to_string(),
                        is_bot: s["is_bot"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let match_id = meta["match_id"]
        .as_str()
        .or(start["run"].as_str())
        .unwrap_or_default()
        .to_string();
    let summary = end.as_ref().and_then(|e| e["summary"]["summary"].as_object().map(|_| e["summary"]["summary"].clone()));
    let rounds = end
        .as_ref()
        .and_then(|e| e["summary"]["rounds"].as_u64())
        .or_else(|| public_state.as_ref().and_then(|p| p["beat"].as_u64()))
        .map(|r| r as u32);
    let input = MatchInput {
        match_id,
        room_code: meta["code"].as_str().map(str::to_string),
        started_at: start["at"].as_i64(),
        ended_at: end.as_ref().and_then(|e| e["at"].as_i64()),
        rules_version: rules,
        beats_total: start["settings"]["beats_total"].as_u64().map(|b| b as u32),
        forked_from: meta["forked_from"].as_str().map(str::to_string),
        seats,
        summary,
        rounds,
        public_state,
        eliminations: elims,
        recording: file_name,
        source: Source::Backfill,
    };
    Ok((Some(input), st))
}

// ---------------------------------------------------------------------------------------------------------------------
// Extraction: one player's slice of a match
// ---------------------------------------------------------------------------------------------------------------------

/// One player's slice of a match, before it becomes rows.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSlice {
    pub seat: u8,
    pub display_name: String,
    pub champion: bool,
    pub placement: Option<u32>,
    pub alive: Option<bool>,
    pub eliminated_beat: Option<u32>,
    pub cause: Option<String>,
    pub score: Option<f64>,
    pub demigods: Vec<MatchDemigod>,
    pub items: Vec<NotableItem>,
    pub deeds: Vec<Deed>,
    pub stats: Value,
    pub eliminations: Vec<Elimination>,
    pub identities: Vec<IdentitySeen>,
    pub images: Vec<ImageSeen>,
}

/// A demigod identity as seen in one match.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentitySeen {
    pub name: String,
    pub hand: Option<String>,
    pub sheet_hash: Option<String>,
    pub traits: Option<Value>,
    pub portrait_hash: Option<String>,
    pub diorama_hash: Option<String>,
}

/// An image made for a player in one match.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSeen {
    pub hash: String,
    pub kind: String,
    pub caption: String,
    pub subject: Option<String>,
    pub subject_name: Option<String>,
}

/// Most deeds kept per player per match.
pub const DEEDS_PER_MATCH: usize = 10;
/// Most notable items kept per player per match.
pub const ITEMS_PER_MATCH: usize = 24;

fn seat_of(v: &Value) -> Option<u8> {
    v["seat"].as_u64().map(|s| s as u8)
}

fn hash_of(img: &Value) -> Option<String> {
    img["hash"].as_str().filter(|h| !h.is_empty()).map(str::to_string)
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// The slices of every human seat in `m` (bots too when `include_bots`). AI demigods (no seat) never get one.
pub fn slices(m: &MatchInput, include_bots: bool) -> Vec<PlayerSlice> {
    let empty = vec![];
    let sum = m.summary.as_ref();
    let standings = sum.and_then(|s| s["standings"].as_array()).unwrap_or(&empty);
    let piles = sum.and_then(|s| s["piles"].as_array()).unwrap_or(&empty);
    let shots = sum.and_then(|s| s["storyboard"].as_array()).unwrap_or(&empty);
    let schema5_players = sum.and_then(|s| s["players"].as_array());
    let pub_dgs = m
        .public_state
        .as_ref()
        .and_then(|p| p["demigods"].as_array())
        .unwrap_or(&empty);
    let champion_seat = sum.and_then(|s| s["champion_seat"].as_u64()).map(|x| x as u8);
    let champion_seats: Vec<u8> = sum
        .and_then(|s| s["champions"].as_array())
        .map(|a| a.iter().filter_map(seat_of).collect())
        .unwrap_or_default();
    let title = sum.and_then(|s| s["final_cut"]["title"].as_str()).unwrap_or("Final Night 7");
    let code = m.room_code.clone().unwrap_or_else(|| m.match_id.chars().take(8).collect());

    let mut out = vec![];
    for seat in m.seats.iter().filter(|s| include_bots || !s.is_bot) {
        let mine = |v: &&Value| seat_of(v) == Some(seat.seat);
        // The seat's demigods: schema-5 `players[].demigods` first, else every standings / public demigod on the seat.
        let p5 = schema5_players.and_then(|ps| ps.iter().find(|p| seat_of(p) == Some(seat.seat)));
        let mut refs: Vec<String> = vec![];
        let mut demigods: Vec<MatchDemigod> = vec![];
        let mut identities: Vec<IdentitySeen> = vec![];
        let mut images: Vec<ImageSeen> = vec![];
        if let Some(dgs) = p5.and_then(|p| p["demigods"].as_array()) {
            for d in dgs {
                let r = s(&d["ref"]).unwrap_or_default();
                refs.push(r.clone());
                if let Some(owner) = s(&d["demigod"]) {
                    refs.push(owner);
                }
                demigods.push(MatchDemigod {
                    r#ref: r,
                    name: s(&d["name"]).unwrap_or_default(),
                    hand: s(&d["hand"]),
                    alive: d["alive"].as_bool(),
                    eliminated_beat: d["eliminated_beat"].as_u64().map(|b| b as u32),
                    cause: s(&d["cause"]),
                    rank: d["rank"].as_u64().map(|r| r as u32),
                    sheet_hash: s(&d["visual"]["sheet_hash"]),
                });
            }
        }
        for st in standings.iter().filter(mine) {
            let r = s(&st["demigod"]).unwrap_or_default();
            if !refs.contains(&r) {
                refs.push(r.clone());
            }
            if p5.is_none() {
                demigods.push(MatchDemigod {
                    r#ref: r,
                    name: s(&st["name"]).unwrap_or_default(),
                    hand: s(&st["hand"]),
                    alive: st["alive"].as_bool(),
                    eliminated_beat: st["eliminated_beat"].as_u64().map(|b| b as u32),
                    cause: s(&st["cause"]),
                    rank: st["rank"].as_u64().map(|r| r as u32),
                    sheet_hash: None,
                });
            }
        }
        // Public demigods on the seat: visual sheets, portraits, dioramas (and the demigods of an unfinished match).
        let identity_src: Vec<&Value> = match p5.and_then(|p| p["demigods"].as_array()) {
            Some(d) => d.iter().collect(),
            None => pub_dgs.iter().filter(mine).collect(),
        };
        for d in identity_src {
            let name = s(&d["name"]).unwrap_or_default();
            let r = s(&d["ref"]);
            if let Some(r) = &r {
                if !refs.contains(r) {
                    refs.push(r.clone());
                }
                if !demigods.iter().any(|x| &x.r#ref == r) {
                    demigods.push(MatchDemigod {
                        r#ref: r.clone(),
                        name: name.clone(),
                        hand: s(&d["hand"]),
                        alive: d["alive"].as_bool(),
                        eliminated_beat: None,
                        cause: None,
                        rank: None,
                        sheet_hash: None,
                    });
                }
            }
            let sheet = s(&d["visual"]["sheet_hash"]);
            if let Some(md) = demigods.iter_mut().find(|x| Some(&x.r#ref) == r.as_ref()) {
                if md.sheet_hash.is_none() {
                    md.sheet_hash = sheet.clone();
                }
            }
            push_img(&mut images, &d["portrait_image"], "portrait", format!("{name}, portrait"), r.clone(), Some(name.clone()));
            push_img(&mut images, &d["diorama_image"], "diorama", format!("{name}, diorama"), r.clone(), Some(name.clone()));
            if !name.is_empty() {
                identities.push(IdentitySeen {
                    name,
                    hand: s(&d["hand"]),
                    sheet_hash: sheet,
                    traits: d["visual"].get("traits").cloned(),
                    portrait_hash: hash_of(&d["portrait_image"]),
                    diorama_hash: hash_of(&d["diorama_image"]),
                });
            }
        }
        // Standings names never seen in a public state (an old recording) are identities too.
        for d in &demigods {
            if !d.name.is_empty() && !identities.iter().any(|i| i.name == d.name) {
                identities.push(IdentitySeen {
                    name: d.name.clone(),
                    hand: d.hand.clone(),
                    sheet_hash: d.sheet_hash.clone(),
                    traits: None,
                    portrait_hash: None,
                    diorama_hash: None,
                });
            }
        }
        let is_mine_ref = |r: &str| refs.iter().any(|x| x == r);
        // Piles: items, stats, the statue.
        let mut items: Vec<NotableItem> = vec![];
        let mut stats = serde_json::Map::new();
        for p in piles.iter().filter(|p| mine(p) || s(&p["demigod"]).is_some_and(|r| is_mine_ref(&r))) {
            let name = s(&p["name"]).unwrap_or_default();
            push_img(&mut images, &p["statue_image"], "statue", format!("{name}'s statue, {title} ({code})"), s(&p["demigod"]), Some(name.clone()));
            for it in p["items"].as_array().into_iter().flatten().chain(p["summons"].as_array().into_iter().flatten()) {
                let r = s(&it["item"]).unwrap_or_default();
                if items.iter().any(|x| x.r#ref == r) || items.len() >= ITEMS_PER_MATCH {
                    continue;
                }
                let image_hash = hash_of(&it["image"]);
                let iname = s(&it["name"]).unwrap_or_default();
                if let Some(h) = &image_hash {
                    if !images.iter().any(|i| &i.hash == h) {
                        images.push(ImageSeen { hash: h.clone(), kind: "item".into(), caption: iname.clone(), subject: Some(r.clone()), subject_name: Some(iname.clone()) });
                    }
                }
                items.push(NotableItem {
                    r#ref: r,
                    name: iname,
                    origin: s(&it["origin"]),
                    kind: s(&it["kind"]),
                    tier: s(&it["tier"]),
                    count: it["count"].as_f64(),
                    state: s(&it["state"]),
                    image_hash,
                });
            }
            for k in ["bolts", "slows"] {
                if let Some(v) = p.get(k) {
                    stats.insert(k.into(), v.clone());
                }
            }
            if let Some(o) = p["stats"].as_object() {
                for (k, v) in o {
                    stats.insert(k.clone(), v.clone());
                }
            }
        }
        // Storyboard shots that show the seat's demigods: deeds and moment cards.
        let mut deeds: Vec<Deed> = vec![];
        for sh in shots {
            let shot_refs: Vec<String> = sh["refs"].as_array().map(|a| a.iter().filter_map(s).collect()).unwrap_or_default();
            let Some(subject) = shot_refs.iter().find(|r| is_mine_ref(r)).cloned() else {
                continue;
            };
            let caption = s(&sh["caption"]).unwrap_or_default();
            let kind = sh["kind"].as_str().unwrap_or("moment");
            let img_kind = sh["image_kind"].as_str().unwrap_or("event");
            push_img(&mut images, &sh["image"], img_kind, caption.clone(), Some(subject), None);
            if kind == "moment" && deeds.len() < DEEDS_PER_MATCH && !deeds.iter().any(|d| d.caption == caption) {
                deeds.push(Deed {
                    beat: sh["beat"].as_u64().unwrap_or(0) as u32,
                    caption,
                    row: s(&sh["row"]),
                    image_hash: hash_of(&sh["image"]),
                });
            }
        }
        // Eliminations: every fall in the match, the seat's own flagged.
        let mut eliminations: Vec<Elimination> = m
            .eliminations
            .iter()
            .map(|e| {
                let r = e["demigod"].as_u64().map(|d| format!("dg:{d}")).or_else(|| s(&e["demigod"])).unwrap_or_default();
                Elimination {
                    own: is_mine_ref(&r),
                    demigod: r,
                    name: s(&e["name"]).unwrap_or_default(),
                    beat: e["beat"].as_u64().unwrap_or(0) as u32,
                    cause: s(&e["cause"]),
                }
            })
            .collect();
        if eliminations.is_empty() {
            for st in standings.iter().filter(|x| x["eliminated_beat"].is_u64()) {
                let r = s(&st["demigod"]).unwrap_or_default();
                eliminations.push(Elimination {
                    own: is_mine_ref(&r),
                    demigod: r,
                    name: s(&st["name"]).unwrap_or_default(),
                    beat: st["eliminated_beat"].as_u64().unwrap_or(0) as u32,
                    cause: s(&st["cause"]),
                });
            }
        }
        eliminations.sort_by_key(|e| e.beat);
        let best = demigods.iter().filter(|d| d.rank.is_some()).min_by_key(|d| d.rank);
        let champion = champion_seat == Some(seat.seat) || champion_seats.contains(&seat.seat);
        let own_fall = eliminations.iter().filter(|e| e.own).max_by_key(|e| e.beat);
        out.push(PlayerSlice {
            seat: seat.seat,
            display_name: seat.display_name.clone(),
            champion,
            placement: best.and_then(|d| d.rank),
            alive: if m.ended() { Some(demigods.iter().any(|d| d.alive == Some(true))) } else { None },
            eliminated_beat: best.and_then(|d| d.eliminated_beat).or(own_fall.map(|e| e.beat)),
            cause: best.and_then(|d| d.cause.clone()).or(own_fall.and_then(|e| e.cause.clone())),
            score: standings.iter().filter(mine).filter_map(|x| x["score"].as_f64()).reduce(f64::max),
            demigods,
            items,
            deeds,
            stats: Value::Object(stats),
            eliminations,
            identities,
            images,
        });
    }
    out
}

/// A match's champion names and field size from its summary.
pub fn champions_of(summary: Option<&Value>) -> (Vec<String>, Option<u8>, Option<u32>) {
    let Some(s) = summary else {
        return (vec![], None, None);
    };
    let names = s["champions"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let field = s["standings"].as_array().map(|a| a.len() as u32);
    (names, s["champion_seat"].as_u64().map(|x| x as u8), field)
}
