//! People and houses (docs/CLIENT_API.md "People and houses"; docs/MEDIA.md "Sprites and buildings").
//!
//! Benito asked for individual sprites for each person so characters can be told apart at a glance
//! a warrior from a fisherman?" and "why are these random people just standing there i want a living breathing world".
//!
//! - **The library.** [`ROLES`] (16) × 3 looks = 48 sprites, [`HOUSE_KINDS`] (8) × [`HOUSE_LOOKS`] (3) = 24 buildings.
//!   Each entry has a stable key (`sprite/<role>/<look>`, `building/<kind>/<look>`) and a fixed subject
//!   ([`subject`]); the server's media layer turns a key into one picture, generated once and cached forever.
//! - **Assignment.** Read-only and deterministic: a person's role comes from their own words (aliases, title, traits),
//!   then their society role (general, soldier, paladin …), then the crowd and quarter they came from, then a hash;
//!   their look from a stable hash of their ref (narrowed by sex words when the words say). A settlement's house
//!   hexes (the earth sim's buildings, [`crate::earth::effects::site`]) get a kind from its level, its coast and
//!   shrines, and a look from its prosperity.
//! - **Daily life.** Every person in the board disc gets `doing`: an event reaction when there is one (an earth
//!   refuge, a mob, their own public act this beat, an army they march with, their crowd's stance, an army passing),
//!   else a routine from their role and the time of day; plus their home and work hexes and a small `group` id for
//!   people doing the same thing in the same place.
//!
//! Nothing here writes state: the same state gives the same answer.

use std::cell::RefCell;
use std::collections::HashMap;

use fn7_model::earth::Refuge;
use fn7_model::hexgrid::{cell_at, people_cell, HexCell, HexLevel};
use fn7_model::seed::hash_str;
use fn7_model::*;
use fn7_society::geo::distance_km;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::queries::{rows, world, T};

/// The 16 sprite roles.
pub const ROLES: [&str; 16] = [
    "warrior", "fisherman", "farmer", "merchant", "priest", "witch", "beauty", "elder", "child", "soldier", "noble",
    "beggar", "sailor", "smith", "hunter", "healer",
];
/// Looks per role (`1..=3`).
pub const LOOKS: u8 = 3;
/// The 8 building kinds.
pub const HOUSE_KINDS: [&str; 8] = ["hut", "cottage", "longhouse", "manor", "temple", "market_stall", "dock", "tower"];
/// The 3 building looks (the settlement's wealth band).
pub const HOUSE_LOOKS: [&str; 3] = ["poor", "common", "rich"];
/// Rings of the board disc (the client's board: 22 rings = 1,519 hexes).
pub const RINGS: i64 = 22;
/// Most people one `game.people` call returns (nearest first).
pub const MAX_PEOPLE: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sex {
    F,
    M,
    Any,
}
use Sex::*;

/// Each role's three looks: who they are at a glance.
const SPRITES: [(&str, [(Sex, &str); 3]); 16] = [
    ("warrior", [
        (F, "a lean young woman warrior with a braided red ponytail, a scarred cheek, boiled-leather armour and a long spear"),
        (M, "a broad bearded warrior in battered chainmail with a round painted shield and a notched longsword"),
        (M, "a towering bald warrior in a horned iron helm and furs, carrying a huge two-handed axe"),
    ]),
    ("fisherman", [
        (M, "a weathered old fisherman with a grey beard, an oilskin coat, rolled trousers and a net over one shoulder"),
        (F, "a sturdy fishwife with rolled sleeves, a headscarf and a basket of silver fish on her hip"),
        (M, "a wiry barefoot young fisherman in a straw hat with a long fishing rod and a string of fish"),
    ]),
    ("farmer", [
        (M, "a sunburnt farmer in a straw hat and a patched smock leaning on a pitchfork"),
        (F, "a strong farm woman in an apron and clogs with a sickle and a sheaf of golden wheat"),
        (M, "a stooped old farmer muddy to the knees with a hoe and a sack of turnips"),
    ]),
    ("merchant", [
        (M, "a plump merchant in a rich burgundy coat with gold rings, a fat coin purse and a ledger under his arm"),
        (F, "a sharp-eyed merchant woman in a green travelling cloak with a laden pack and brass scales"),
        (M, "a thin spice trader in silk robes and a turban holding a small open chest of spices"),
    ]),
    ("priest", [
        (M, "an elderly priest in long white and gold vestments swinging a smoking censer"),
        (F, "a serene young priestess in a hooded blue robe with a glowing holy symbol on a chain"),
        (M, "a gaunt hooded monk in a rough brown habit with a rope belt and a heavy prayer book"),
    ]),
    ("witch", [
        (F, "an ugly old witch with a hooked nose, warts, wild grey hair, a crooked pointed hat and a gnarled staff"),
        (F, "a pale young witch in a tattered black dress with a raven on her shoulder and a glowing green vial"),
        (F, "a hunched hedge witch draped in moss-green shawls with bundles of herbs and little bones hanging from her belt"),
    ]),
    ("beauty", [
        (F, "a strikingly beautiful young woman with long dark wavy hair, a flowing red gown and a flower behind her ear"),
        (F, "a gorgeous golden-haired maiden in an elegant pale blue dress with a silver circlet"),
        (M, "a dashing handsome young man with tousled hair, an open-collared white shirt and a confident grin"),
    ]),
    ("elder", [
        (M, "a frail white-bearded elder in a long grey robe leaning on a carved walking stick"),
        (F, "a wrinkled grandmother in a shawl with a silver bun and kind eyes, holding a small lantern"),
        (M, "a dignified old village chief in a fur mantle with a wooden staff and a necklace of teeth"),
    ]),
    ("child", [
        (M, "a scrappy little boy with a dirty face and an oversized tunic, waving a wooden toy sword"),
        (F, "a small girl with pigtails and a patched dress, hugging a rag doll"),
        (Any, "a grinning barefoot street urchin in a flat cap holding a stolen apple"),
    ]),
    ("soldier", [
        (M, "a helmeted soldier in a tabard over mail with a tall spear and a kite shield"),
        (M, "a nervous young recruit in an ill-fitting helmet and a padded jacket with a crossbow"),
        (F, "a stern woman sergeant in plate pauldrons with a short sword and a signal horn"),
    ]),
    ("noble", [
        (M, "a haughty lord in a velvet doublet with a fur-trimmed cape, a gold chain and a rapier"),
        (F, "a regal noblewoman in an embroidered purple gown with jewels and a tall lace collar"),
        (M, "a fat bejewelled baron in brocade with a feathered hat and a goblet of wine"),
    ]),
    ("beggar", [
        (M, "a ragged beggar in torn sackcloth with a wooden bowl and a crutch"),
        (F, "a hunched old beggar woman wrapped in filthy shawls, holding out one hand"),
        (M, "a one-eyed vagabond in a patched cloak with a bindle on a stick"),
    ]),
    ("sailor", [
        (M, "a tattooed sailor in a striped shirt and a bandana with a coil of rope over his shoulder"),
        (M, "a peg-legged old sea captain in a long blue coat and a tricorn hat with a brass spyglass"),
        (F, "a bold young woman sailor with rolled sleeves, a headscarf and a cutlass at her hip"),
    ]),
    ("smith", [
        (M, "a muscular blacksmith in a leather apron with soot-stained arms, a hammer and tongs"),
        (M, "a stout smith with a braided beard and goggles holding a glowing red horseshoe in tongs"),
        (F, "a brawny woman smith with cropped hair and a heavy apron holding a freshly forged blade"),
    ]),
    ("hunter", [
        (M, "a hooded hunter in green and brown leathers with a longbow and a quiver of arrows"),
        (F, "a lithe huntress in a fur cloak with a short bow and a hare hanging from her belt"),
        (M, "a grizzled trapper in a wolf-pelt hood with a boar spear and coils of snares"),
    ]),
    ("healer", [
        (F, "a gentle healer in a white apron with a satchel of bandages and little herb bottles"),
        (M, "a bespectacled physician in a dark coat with a leather bag and a beaked plague mask on his belt"),
        (F, "a young herbalist with a flower crown, a mortar and pestle and a basket of herbs"),
    ]),
];

/// Each building kind's three looks (poor, common, rich).
const BUILDINGS: [(&str, [&str; 3]); 8] = [
    ("hut", [
        "a tiny round mud-and-wattle hut with a sagging thatched roof and a smoke hole",
        "a sturdy round timber hut with a neat thatched cone roof and a hide door",
        "a large round hut with painted walls, a double thatched roof and carved door posts",
    ]),
    ("cottage", [
        "a crooked one-room stone cottage with a patched thatched roof and a broken fence",
        "a cosy timber-framed cottage with whitewashed walls, a thatched roof, a chimney and a flower box",
        "a two-storey cottage with a slate roof, glass windows, climbing ivy and a little walled garden",
    ]),
    ("longhouse", [
        "a long low longhouse of rough logs with a turf roof",
        "a long timber longhouse with a steep shingled roof and carved dragon gables",
        "a great hall longhouse with painted carved beams, shields along its walls and banners",
    ]),
    ("manor", [
        "a faded old manor house with boarded windows and a crumbling garden wall",
        "a stone manor house with a tiled roof, tall chimneys and a walled courtyard",
        "a grand manor with corner towers, a blue slate roof, glowing windows and a gilded gate",
    ]),
    ("temple", [
        "a small roadside shrine-temple of rough stones with a single candle niche",
        "a stone chapel with a bell tower and a round rose window",
        "a marble temple with columns, a golden dome and braziers burning at its steps",
    ]),
    ("market_stall", [
        "a rickety market stall of stacked crates under a torn cloth awning",
        "a wooden market stall with a striped awning and baskets of produce",
        "a covered merchant's pavilion with silk awnings, hanging lanterns and stacked goods",
    ]),
    ("dock", [
        "a rotting wooden jetty with a leaning fishing shack and a small rowboat",
        "a wooden dock with a boathouse, nets drying on poles and barrels",
        "a stone harbour quay with a wooden crane, a warehouse and a moored sailing ship",
    ]),
    ("tower", [
        "a squat ruined watchtower of rough stone with a wooden lookout",
        "a round stone watchtower with a conical roof and a torch burning at the top",
        "a tall slender tower of pale stone with spires, glowing windows and banners",
    ]),
];

/// `sprite/<role>/<look>` (look 1..=3).
pub fn sprite_key(role: &str, look: u8) -> String {
    format!("sprite/{role}/{look}")
}

/// `building/<kind>/<look>` (look poor | common | rich).
pub fn building_key(kind: &str, look: &str) -> String {
    format!("building/{kind}/{look}")
}

/// Every library key, sprites first (48 + 24 = 72).
pub fn library_keys() -> Vec<String> {
    let mut v: Vec<String> = ROLES.iter().flat_map(|r| (1..=LOOKS).map(move |l| sprite_key(r, l))).collect();
    v.extend(HOUSE_KINDS.iter().flat_map(|k| HOUSE_LOOKS.iter().map(move |l| building_key(k, l))));
    v
}

/// The image kind (`sprite` | `building`) and the subject of a library key; `None` for anything else. The subject
/// reads `<role> (look <look> of 3): <who they are>` (the asset registry keys on that head).
pub fn subject(key: &str) -> Option<(&'static str, String)> {
    let mut it = key.split('/');
    let (kind, id, look) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    match kind {
        "sprite" => {
            let n: usize = look.parse().ok().filter(|n| (1..=LOOKS as usize).contains(n))?;
            let (_, looks) = SPRITES.iter().find(|(r, _)| *r == id)?;
            Some(("sprite", format!("{id} (look {n} of 3): {}, one of the people of Final Night 7's world", looks[n - 1].1)))
        }
        "building" => {
            let i = HOUSE_LOOKS.iter().position(|l| *l == look)?;
            let (_, looks) = BUILDINGS.iter().find(|(k, _)| *k == id)?;
            Some(("building", format!("{} (look {look} of 3): {}", id.replace('_', " "), looks[i])))
        }
        _ => None,
    }
}

/// The sprite object the plugin writes: `{key, kind, status: "off", url: null, hash: null}`. The server's media
/// layer fills `hash`, `status` and `url` (docs/MEDIA.md "Sprites and buildings"); the bare plugin leaves it off.
pub fn query_sprites(args: &Value) -> Result<Value, String> {
    let kind = args.get("kind").and_then(Value::as_str).unwrap_or("all");
    if !["people", "buildings", "all"].contains(&kind) {
        return Err(format!("kind must be people, buildings or all, not {kind}"));
    }
    let items: Vec<Value> = library_keys()
        .into_iter()
        .filter(|k| match kind { "people" => k.starts_with("sprite/"), "buildings" => k.starts_with("building/"), _ => true })
        .map(|k| {
            let mut it = k.split('/');
            let (lib, id, look) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
            let mut v = json!({ "key": k, "look": look, "sprite": sprite_stub(&k) });
            v[if lib == "sprite" { "role" } else { "building" }] = json!(id);
            v
        })
        .collect();
    Ok(json!({ "kind": kind, "total": items.len(), "items": items }))
}

pub fn sprite_stub(key: &str) -> Value {
    let kind = key.split('/').next().unwrap_or("sprite");
    json!({ "key": key, "kind": kind, "status": "off", "url": null, "hash": null })
}

// ---------------------------------------------------------------------------------------------------------------------
// Role and look
// ---------------------------------------------------------------------------------------------------------------------

/// Words → role, first match wins (a `*` suffix matches any word starting so; several words match as a phrase).
const ROLE_WORDS: &[(&str, &[&str])] = &[
    ("witch", &["witch*", "hag", "crone", "sorceress", "warlock", "medium", "seer", "fortune teller", "soothsayer"]),
    ("beauty", &["beauty", "beautiful", "maiden", "gorgeous", "pretty", "handsome", "courtesan", "dancer", "lovely", "hot", "belle", "charming", "seductive"]),
    ("child", &["child", "children", "boy", "girl", "kid", "kids", "urchin*", "orphan*", "pickpocket*", "street kid", "lad", "lass", "youngster"]),
    ("fisherman", &["fisher*", "fishwife", "fishwives", "net-mender*", "net", "nets", "trawler", "angler", "fishmonger"]),
    ("sailor", &["sailor*", "stevedore*", "dockworker*", "docker*", "bargem*", "bargewom*", "ferry*", "harbormaster", "harbourmaster", "smuggler*", "boatman", "mariner", "lockkeeper", "deckhand", "pirate*"]),
    ("healer", &["healer*", "nurse*", "surgeon*", "apothecar*", "physician*", "doctor*", "herbalist*", "orderl*", "midwife", "medic"]),
    ("priest", &["priest*", "abbess", "abbot", "monk*", "nun", "nuns", "cleric*", "clergy", "verger*", "acolyte*", "prophet*", "preacher*", "pilgrim*", "penitent*", "bell-ringer", "paladin*", "doomsayer*", "oracle", "zealot*", "cultist*"]),
    ("smith", &["smith*", "blacksmith*", "forge*", "foundry", "furnace", "mason*", "clockmaker*", "craftsm*", "builder*", "foreman", "scaffolder*", "tinker*", "carpenter*"]),
    ("hunter", &["hunter*", "huntress", "trapper*", "archer*", "ranger*", "poacher*", "gamekeeper*", "tracker*"]),
    ("farmer", &["farmer*", "farm", "peasant*", "shepherd*", "miller*", "baker*", "gardener*", "beekeeper*", "herdsm*", "milkmaid*", "ploughm*", "plowm*"]),
    ("noble", &["noble*", "lord", "lady", "duke", "duchess", "baron*", "count", "countess", "prince", "princess", "king", "queen", "dowager*", "heir", "heirs", "heiress", "alderman", "dean", "impresario", "slumlord", "aristocrat*", "nobility"]),
    ("merchant", &["merchant*", "trader*", "vendor*", "stallholder*", "shopkeeper*", "grocer*", "clerk*", "importer*", "spice", "banker*", "moneylender*", "tax collector*", "innkeeper*", "peddler*"]),
    ("elder", &["elder*", "old man", "old woman", "grandmother", "grandfather", "granny", "widow*", "pensioner*", "mourner*", "wise woman", "wise man"]),
    ("soldier", &["soldier*", "guard*", "watchm*", "watch", "recruit*", "sentry", "sentries", "patrol*", "militia*", "quartermaster", "bodyguard*", "garrison", "infantry", "conscript*"]),
    ("warrior", &["warrior*", "general", "captain", "champion*", "knight*", "mercenar*", "fighter*", "veteran*", "hero*", "avenger*", "berserker*", "gladiator*", "duelist*", "bandit*", "brawler*"]),
    ("beggar", &["beggar*", "vagabond*", "tramp*", "outcast*", "day laborer*", "laborer*", "labourer*", "deserter*", "coward*", "gambler*", "card sharp*", "drunk*", "vagrant*"]),
];

const FEMALE: &[&str] = &[
    "woman", "women", "girl", "she", "her", "maiden", "wife", "fishwife", "fishwives", "widow", "lady", "queen", "princess",
    "duchess", "abbess", "nun", "priestess", "huntress", "sorceress", "bargewoman", "mother", "grandmother", "granny",
    "daughter", "sister", "dowager", "matron", "seamstress", "heiress", "baroness", "countess", "milkmaid", "lass",
    "witch", "hag", "crone", "belle", "midwife",
];
const MALE: &[&str] = &[
    "man", "men", "boy", "he", "his", "husband", "widower", "lord", "king", "prince", "duke", "baron", "abbot", "monk",
    "father", "grandfather", "son", "brother", "fisherman", "bargeman", "watchman", "foreman", "alderman", "lad",
    "warlock", "boatman", "herdsman", "ploughman",
];

/// `" a b c "`: lowercase words split on anything that is not a letter, digit or hyphen.
fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(' ');
    for ch in s.chars() {
        if ch.is_alphanumeric() || ch == '-' {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// A text's words, once (no allocation per keyword).
struct Words<'t> {
    /// `" a b c "`.
    text: &'t str,
    tokens: Vec<&'t str>,
}

impl<'t> Words<'t> {
    fn new(text: &'t str) -> Self {
        Self { text, tokens: text.split(' ').filter(|t| !t.is_empty()).collect() }
    }
    /// `kw`: a word, a stem with `*` (any word starting so), or a phrase of several words.
    fn has(&self, kw: &str) -> bool {
        if let Some(stem) = kw.strip_suffix('*') {
            self.tokens.iter().any(|t| t.starts_with(stem))
        } else if kw.contains(' ') {
            self.text.match_indices(kw).any(|(i, _)| {
                self.text[..i].ends_with(' ') && self.text[i + kw.len()..].starts_with(' ')
            })
        } else {
            self.tokens.iter().any(|t| *t == kw)
        }
    }
}

/// The role words in `text` name, if any.
fn role_from_words(text: &str) -> Option<&'static str> {
    let t = norm(text);
    let ws = Words::new(&t);
    ROLE_WORDS.iter().find(|(_, kws)| kws.iter().any(|k| ws.has(k))).map(|(r, _)| *r)
}

fn sex_from_words(text: &str) -> Sex {
    let t = norm(text);
    let ws = Words::new(&t);
    let f = FEMALE.iter().any(|k| ws.has(k));
    let m = MALE.iter().any(|k| ws.has(k));
    match (f, m) {
        (true, false) => F,
        (false, true) => M,
        _ => Any,
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// A stable hash of a ref and a salt (no match seed: the same ref gets the same pick).
fn h(r: &str, salt: u64) -> u64 {
    mix(hash_str(r) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// A person's own words: aliases (how the story named them), their rank title and traits.
fn own_words(x: &Character) -> String {
    let mut s = x.aliases.join(" . ");
    if let Some(l) = &x.life {
        s.push_str(" . ");
        s.push_str(&l.rank.title);
    }
    s.push_str(" . ");
    s.push_str(&x.traits.join(" . "));
    s
}

/// Where a person came from: their crowd's label and groups, and their quarter's archetype and name.
fn origin_words(w: &World, x: &Character) -> String {
    let mut s = String::new();
    if let Origin::Materialized { cluster, .. } = x.origin {
        if let Some(c) = w.clusters.get(cluster.ix()) {
            s.push_str(&c.label);
            for g in c.groups.as_slice() {
                if let Some(g) = w.groups.get(g.ix()) {
                    s.push_str(" . ");
                    s.push_str(&g.name);
                }
            }
        }
    }
    if let Some(p) = w.places.get(x.place.ix()) {
        s.push_str(" . ");
        s.push_str(&p.archetype.clone().unwrap_or_default().replace('_', " "));
    }
    s
}

/// A person's sprite role: their own words (a "witch", a "fisherman", a "maiden"), then their society role (general
/// → warrior, soldier → soldier, paladin and prophet → priest, ringleader → beggar or warrior …), then the crowd and
/// quarter they came from, then their notability, then a hash (a coast-flavoured pick by the sea).
pub fn role_of(w: &World, x: &Character) -> &'static str {
    let r = Ref::Char(x.id).to_string();
    if let Some(role) = role_from_words(&own_words(x)) {
        return role;
    }
    if let Some(l) = &x.life {
        let pick = |a: &'static str, b: &'static str| if h(&r, 11) % 2 == 0 { a } else { b };
        match l.role {
            Role::General | Role::Hero | Role::Avenger => return "warrior",
            Role::Captain => return pick("warrior", "soldier"),
            Role::Soldier => return "soldier",
            Role::Paladin | Role::Prophet => return "priest",
            Role::Ringleader => return pick("beggar", "warrior"),
            Role::Deserter => return pick("beggar", "soldier"),
            _ => {}
        }
        if l.army.is_some() {
            return "soldier";
        }
    }
    if let Some(role) = role_from_words(&origin_words(w, x)) {
        return role;
    }
    if matches!(x.origin, Origin::Seeded) || x.life.as_ref().is_some_and(|l| l.role == Role::Notable) {
        return ["noble", "merchant", "elder", "priest"][(h(&r, 12) % 4) as usize];
    }
    const TOWN: [&str; 8] = ["farmer", "merchant", "smith", "farmer", "hunter", "healer", "elder", "child"];
    TOWN[(h(&r, 13) % TOWN.len() as u64) as usize]
}

/// [`role_of`] and [`look_of`], remembered per process by a hash of everything they read (the words, the society role,
/// the army, the origin, the ref): a board poll pays the word scans once per person, and a changed person (a new alias,
/// a promotion) is a new key, so the answer is always exactly what the two functions give.
pub fn assign(w: &World, x: &Character) -> (&'static str, u8) {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<HashMap<u64, (&'static str, u8)>>> = std::sync::OnceLock::new();
    let life = x.life.as_ref().map(|l| format!("{:?}|{}", l.role, l.army.is_some())).unwrap_or_default();
    let key = mix(hash_str(&format!(
        "{}|{}|{}|{life}|{}|{}",
        Ref::Char(x.id),
        own_words(x),
        origin_words(w, x),
        matches!(x.origin, Origin::Seeded),
        x.name
    )));
    let m = SEEN.get_or_init(Default::default);
    if let Some(v) = m.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return *v;
    }
    let role = role_of(w, x);
    let v = (role, look_of(x, role));
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    if g.len() > 200_000 {
        g.clear();
    }
    g.insert(key, v);
    v
}

/// A person's look (1..=3): a stable hash of their ref, among the role's looks that fit the sex their words say.
pub fn look_of(x: &Character, role: &str) -> u8 {
    let r = Ref::Char(x.id).to_string();
    let looks = SPRITES.iter().find(|(k, _)| *k == role).map(|x| x.1).unwrap_or(SPRITES[0].1);
    let sex = sex_from_words(&format!("{} . {}", own_words(x), x.name));
    let fits: Vec<u8> = (0..3u8).filter(|i| sex == Any || looks[*i as usize].0 == sex || looks[*i as usize].0 == Any).map(|i| i + 1).collect();
    let pool: Vec<u8> = if fits.is_empty() { vec![1, 2, 3] } else { fits };
    pool[(h(&r, 21) % pool.len() as u64) as usize]
}

// ---------------------------------------------------------------------------------------------------------------------
// Buildings
// ---------------------------------------------------------------------------------------------------------------------

/// One house hex of a settlement.
#[derive(Debug, Clone, PartialEq)]
pub struct House {
    pub cell: HexCell,
    pub kind: &'static str,
    pub look: &'static str,
    pub place: PlaceId,
}

fn look_band(prosperity: f32, level: u8) -> usize {
    let mut i: i32 = if prosperity < 0.35 { 0 } else if prosperity < 0.7 { 1 } else { 2 };
    if level >= 5 {
        i += 1;
    }
    if level <= 1 {
        i -= 1;
    }
    i.clamp(0, 2) as usize
}

/// [`crate::wills::quests::is_coast`], remembered per position for the process (cities stand where they stand, so a
/// board poll never pays for the land lookups twice).
pub fn coastal(w: &World, at: LonLat) -> bool {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<HashMap<(u64, u64, bool), bool>>> = std::sync::OnceLock::new();
    let key = (at[0].to_bits(), at[1].to_bits(), w.is_planet());
    let m = SEEN.get_or_init(Default::default);
    if let Some(v) = m.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return *v;
    }
    let v = crate::wills::quests::is_coast(w, at);
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    if g.len() < 100_000 {
        g.insert(key, v);
    }
    v
}

/// A settlement's house hexes with their kinds and looks: the earth sim's buildings of the place
/// ([`crate::earth::effects::site`], 3 + level of them, at most 9), in its order. The special buildings come first:
/// a dock on a coast, a temple where a shrine stands (or from level 3), a market stall from level 2, a manor from level
/// 3 or when rich, a tower from level 4; the rest are dwellings (huts in a hamlet, cottages, longhouses in the north
/// and in bigger towns). The look is the settlement's wealth band (prosperity, nudged by its level); one dwelling in
/// four is a band poorer; a manor is never poor.
pub fn houses(w: &World, p: &Place) -> Vec<House> {
    let site = crate::earth::effects::site(w, p);
    let st = w.dm.settlement(p.id);
    let level = st.map(|s| s.level).unwrap_or(site.level);
    let prosperity = st.map(|s| s.prosperity).unwrap_or(0.5);
    let band = look_band(prosperity, level);
    let near = |at: LonLat| distance_km(at, p.center) <= (p.radius_km * 1.5).max(if w.is_planet() { 15.0 } else { 0.5 });
    let coast = coastal(w, p.center);
    let shrine = w.dm.shrines.iter().any(|s| s.place == p.id || near(s.at));
    let mut special: Vec<&'static str> = vec![];
    if coast {
        special.push("dock");
    }
    if shrine || level >= 3 {
        special.push("temple");
    }
    if level >= 2 {
        special.push("market_stall");
    }
    if level >= 3 || prosperity >= 0.7 {
        special.push("manor");
    }
    if level >= 4 {
        special.push("tower");
    }
    let north = p.center[1].abs() >= 50.0;
    site.buildings
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let hc = mix(hash_str(&c.key()) ^ w.meta.seed);
            let kind = match special.get(i) {
                Some(k) => *k,
                None if level <= 1 => "hut",
                None if north && hc % 2 == 0 => "longhouse",
                None if level >= 3 && hc % 3 == 0 => "longhouse",
                None if level == 2 && hc % 3 == 0 => "hut",
                None => "cottage",
            };
            let mut b = band;
            if special.get(i).is_none() && (hc >> 8) % 4 == 0 {
                b = b.saturating_sub(1);
            }
            if kind == "manor" {
                b = b.max(1);
            }
            House { cell: *c, kind, look: HOUSE_LOOKS[b], place: p.id }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------------
// Daily life
// ---------------------------------------------------------------------------------------------------------------------

/// What one `game.people` call precomputes once.
pub(crate) struct Ctx<'a> {
    w: &'a World,
    beat: u32,
    phase: &'static str,
    /// The latest beat's public acts by person: (verb, first target).
    acts: HashMap<CharId, (String, Option<Ref>)>,
    /// Open earth refuges by crowd.
    refuges: HashMap<u32, Refuge>,
    /// Each settlement's houses (computed on first need).
    houses: RefCell<HashMap<PlaceId, Vec<House>>>,
    /// The live armies: (label, position, demigod).
    armies: Vec<(&'a str, LonLat, DemigodId)>,
}

#[derive(Deserialize)]
struct ActHead<'a> {
    beat: u32,
    verb: &'a str,
    magnitude: u8,
    source: Source,
    actor: Ref,
    #[serde(default)]
    targets: Vec<Ref>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(w: &'a World, state: &Value) -> Self {
        let st = crate::world::settings(w);
        let (_, phase, _, _) = crate::dm::phase_of(w.meta.beat.max(1), st.beats_per_day);
        // The latest beat's public acts (at most SCAN_CAP rows, newest first: the newest act of a person wins).
        let mut acts = HashMap::new();
        let all = rows(state);
        let last = all.last().and_then(|v| v.get("beat")).and_then(Value::as_u64).unwrap_or(0) as u32;
        if last + 1 >= w.meta.beat {
            for v in all.iter().rev().take(crate::queries::SCAN_CAP) {
                let Ok(hd) = ActHead::deserialize(v) else { continue };
                if hd.beat != last {
                    break;
                }
                if let Ref::Char(c) = hd.actor {
                    if !acts.contains_key(&c) && crate::payload::is_public_head(w, hd.verb, hd.magnitude, &hd.source, hd.actor) {
                        acts.insert(c, (hd.verb.to_string(), hd.targets.first().copied()));
                    }
                }
            }
        }
        let refuges = w
            .dm
            .earth
            .as_ref()
            .map(|e| e.refuges.iter().filter(|r| r.until == 0).map(|r| (r.crowd, r.clone())).collect())
            .unwrap_or_default();
        let armies = fn7_society::armies(w).map(|(k, a)| (k.label.as_str(), a.pos, a.demigod)).collect();
        Self { w, beat: w.meta.beat, phase, acts, refuges, houses: RefCell::new(HashMap::new()), armies }
    }

    pub(crate) fn phase(&self) -> &'static str {
        self.phase
    }

    fn houses_of(&self, p: PlaceId) -> Vec<House> {
        if let Some(v) = self.houses.borrow().get(&p) {
            return v.clone();
        }
        let v = self.w.places.get(p.ix()).filter(|x| x.kind == PlaceKind::Site).map(|x| houses(self.w, x)).unwrap_or_default();
        self.houses.borrow_mut().insert(p, v.clone());
        v
    }

    /// The settlement a person lives in: up their place chain, else the one around where they stand.
    fn settlement_of(&self, x: &Character) -> Option<PlaceId> {
        let mut p = Some(x.place);
        for _ in 0..6 {
            let Some(id) = p else { break };
            let pl = self.w.places.get(id.ix())?;
            if pl.kind == PlaceKind::Site {
                return Some(id);
            }
            p = pl.parent;
        }
        crate::dm::settlement_at(self.w, x.pos)
    }
}

/// What a role does at work, where, and whether it walks about doing it.
fn routine(role: &str) -> (&'static str, &'static str, &'static str, bool) {
    // (activity, doing, work, moving)
    match role {
        "warrior" => ("training", "training with a blade", "wall", false),
        "fisherman" => ("fishing", "fishing off the dock", "dock", false),
        "farmer" => ("farming", "working the fields", "field", false),
        "merchant" => ("trading", "haggling at the market", "market", false),
        "priest" => ("praying", "leading prayers at the temple", "temple", false),
        "witch" => ("brewing", "brewing something foul", "home", false),
        "beauty" => ("strolling", "strolling through the market", "market", true),
        "elder" => ("storytelling", "telling old stories", "home", false),
        "child" => ("playing", "playing in the street", "home", true),
        "soldier" => ("patrolling", "patrolling the walls", "wall", true),
        "noble" => ("holding_court", "holding court", "manor", false),
        "beggar" => ("begging", "begging for coins", "market", false),
        "sailor" => ("sailing", "mending sails at the dock", "dock", false),
        "smith" => ("smithing", "hammering at the forge", "forge", false),
        "hunter" => ("hunting", "out hunting past the fields", "field", true),
        "healer" => ("healing", "tending the sick", "temple", false),
        _ => ("resting", "resting", "home", false),
    }
}

/// `verb` as "-ing" ("strike" → "striking", "rob" → "robbing").
fn gerund(verb: &str) -> String {
    let v = verb.trim().to_ascii_lowercase().replace('_', " ");
    let (head, tail) = match v.split_once(' ') {
        Some((a, b)) => (a.to_string(), format!(" {b}")),
        None => (v.clone(), String::new()),
    };
    let g = if head.ends_with("ie") {
        format!("{}ying", &head[..head.len() - 2])
    } else if head.ends_with('e') && !head.ends_with("ee") && head.len() > 2 {
        format!("{}ing", &head[..head.len() - 1])
    } else if matches!(head.as_str(), "rob" | "stab" | "hit" | "cut" | "run" | "sit" | "beg" | "grab" | "trap" | "spit" | "chat" | "plan" | "shop" | "stop" | "drop" | "swim" | "win" | "dig" | "rip" | "slap" | "mug" | "nag" | "hug" | "kidnap") {
        format!("{head}{}ing", head.chars().last().unwrap_or('x'))
    } else {
        format!("{head}ing")
    };
    format!("{g}{tail}")
}

/// What a person is doing now.
struct Now {
    activity: &'static str,
    doing: String,
    moving: bool,
    to_cell: Option<String>,
    why: &'static str,
}

/// One person as `game.people` lists them (docs/CLIENT_API.md "People and houses"). `group` is filled in by
/// [`assign_groups`].
pub(crate) fn person_item(cx: &Ctx, x: &Character) -> Value {
    item_of(cx, x, None)
}

/// A named person (`resident: None`), or a resident of a house (`Some((key, house))`: one of the people living in it,
/// drawn from a crowd of its settlement, keyed by `key` and listed with `ref: null`).
fn item_of(cx: &Ctx, x: &Character, resident: Option<(&str, &House)>) -> Value {
    let w = cx.w;
    let r = resident.map(|(k, _)| k.to_string()).unwrap_or_else(|| Ref::Char(x.id).to_string());
    let (role, look) = assign(w, x);
    let cell = people_cell(x.pos);
    let settlement = resident.map(|(_, b)| b.place).or_else(|| cx.settlement_of(x));
    let houses = settlement.map(|p| cx.houses_of(p)).unwrap_or_default();
    let dwellings: Vec<&House> = houses.iter().filter(|b| matches!(b.kind, "hut" | "cottage" | "longhouse" | "manor")).collect();
    let home: Option<&House> = resident
        .map(|(_, b)| b)
        .or_else(|| if role == "noble" { houses.iter().find(|b| b.kind == "manor") } else { None })
        .or_else(|| (!dwellings.is_empty()).then(|| dwellings[(h(&r, 31) % dwellings.len() as u64) as usize]))
        .or_else(|| (!houses.is_empty()).then(|| &houses[(h(&r, 31) % houses.len() as u64) as usize]));
    let home_cell = home.map(|b| b.cell.key());
    let (activity, routine_doing, work, routine_moves) = routine(role);
    let work_cell: Option<String> = settlement.and_then(|p| {
        let want = match work {
            "dock" => Some("dock"),
            "market" => Some("market_stall"),
            "temple" => Some("temple"),
            "manor" => Some("manor"),
            "wall" => Some("tower"),
            _ => None,
        };
        let center = cell_at(w.places[p.ix()].center, HexLevel::Houses);
        match work {
            "home" => home_cell.clone(),
            // Fields lie four rings out, the walls three: a stable spot for each person.
            "field" | "wall" if want.and_then(|k| houses.iter().find(|b| b.kind == k)).is_none() => {
                let ring = center.ring(if work == "field" { 4 } else { 3 });
                Some(ring[(h(&r, 32) % ring.len() as u64) as usize].key())
            }
            _ => want
                .and_then(|k| houses.iter().find(|b| b.kind == k))
                .or_else(|| (!houses.is_empty()).then(|| &houses[(h(&r, 33) % houses.len() as u64) as usize]))
                .map(|b| b.cell.key()),
        }
    });
    let mut now = Now { activity, doing: routine_doing.to_string(), moving: routine_moves, to_cell: None, why: "routine" };
    let crowd = match x.origin {
        Origin::Materialized { cluster, .. } => w.clusters.get(cluster.ix()),
        _ => None,
    };
    let set = |a: &'static str, d: String, m: bool, t: Option<String>, why: &'static str| Now { activity: a, doing: d, moving: m, to_cell: t, why };
    let mob = x.life.as_ref().and_then(|l| l.mob).and_then(|m| w.mobs.get(m.ix())).filter(|m| m.ended.is_none());
    let army = x.life.as_ref().and_then(|l| l.army).and_then(|k| w.clusters.get(k.ix())).filter(|k| k.army.is_some());
    let refuge = crowd.and_then(|c| cx.refuges.get(&c.id.0));
    let scale = if w.is_planet() { 1.0 } else { 0.01 };
    if let Some(rf) = refuge {
        now = if rf.since >= cx.beat.saturating_sub(1) {
            set("fleeing", "fleeing to shelter".into(), true, Some(rf.at.clone()), "earth")
        } else {
            set("sheltering", "sheltering from the storm".into(), false, Some(rf.at.clone()), "earth")
        };
    } else if let Some(m) = mob {
        now = set("rioting", format!("rioting with {}", m.name), true, Some(people_cell(m.pos)), "mob");
    } else if let Some((verb, target)) = cx.acts.get(&x.id) {
        let to = target.map(|t| w.name_of(t)).filter(|n| !n.is_empty());
        let d = match to {
            Some(n) => format!("{} {n}", gerund(verb)),
            None => gerund(verb),
        };
        now = set("acting", d, true, None, "act");
    } else if let Some((k, a)) = army.and_then(|k| k.army.as_ref().map(|a| (k, a))) {
        now = set("marching", format!("marching with {}", k.label), a.dest.is_some(), a.dest.map(people_cell), "army");
    } else if let Some(c) = crowd.filter(|c| matches!(c.stance, Stance::Rioting | Stance::Fleeing | Stance::Worshipping | Stance::Fighting)) {
        now = match c.stance {
            Stance::Rioting => set("rioting", "rioting in the streets".into(), true, None, "crowd"),
            Stance::Fleeing => set("fleeing", "fleeing with the crowd".into(), true, home_cell.clone(), "crowd"),
            Stance::Worshipping => set("praying", "praying with the crowd".into(), false, None, "crowd"),
            _ => set("fighting", "fighting in the streets".into(), true, None, "crowd"),
        };
    } else if let Some((label, _, dg)) = cx.armies.iter().find(|(_, at, _)| distance_km(*at, x.pos) <= 10.0 * scale) {
        let love = crowd.and_then(|c| c.allegiance.iter().find(|(d, _)| d == dg).map(|x| x.1)).unwrap_or(0.0);
        if love >= 0.2 {
            now = set("cheering", format!("cheering {label}"), false, None, "army");
        } else if love <= -0.1 {
            now = set("fleeing", format!("fleeing from {label}"), true, home_cell.clone(), "army");
        }
    } else if x.life.as_ref().is_some_and(|l| matches!(l.goal, Goal::Flee)) {
        now = set("fleeing", "running away".into(), true, home_cell.clone(), "goal");
    }
    if now.why == "routine" {
        let hr = h(&r, cx.beat as u64 ^ 0x51);
        match cx.phase {
            "night" => {
                now = match role {
                    "soldier" => set("patrolling", "on the night watch".into(), true, work_cell.clone(), "routine"),
                    "witch" => set("brewing", "brewing by moonlight".into(), false, None, "routine"),
                    _ if hr % 9 == 0 && role != "child" => set("chatting", "talking late by the fire".into(), false, home_cell.clone(), "routine"),
                    _ => set("sleeping", "asleep".into(), false, home_cell.clone(), "routine"),
                }
            }
            "dawn" => match role {
                "child" => now = set("sleeping", "asleep".into(), false, home_cell.clone(), "routine"),
                "elder" | "noble" | "beauty" => now = set("resting", "taking the morning air".into(), false, home_cell.clone(), "routine"),
                _ if work_cell.is_some() && work_cell != home_cell => {
                    now = set("going_to_work", format!("on the way to the {}", work.replace("market", "market square")), true, work_cell.clone(), "routine")
                }
                _ => {}
            },
            "dusk" => {
                now = match hr % 3 {
                    0 => set("going_home", "heading home".into(), true, home_cell.clone(), "routine"),
                    1 => set("chatting", "chatting with the neighbours".into(), false, home_cell.clone(), "routine"),
                    _ => set("resting", "resting after the day".into(), false, home_cell.clone(), "routine"),
                }
            }
            _ => {
                if hr % 5 == 0 && role != "child" {
                    now = set("chatting", "chatting at work".into(), false, work_cell.clone(), "routine");
                } else if now.moving && work_cell.is_some() {
                    now.to_cell = work_cell.clone();
                }
            }
        }
    }
    let Now { activity, doing, moving, to_cell, why } = now;
    let (rf, name) = match resident {
        Some((_, b)) => (Value::Null, format!("{} of the {}", role, crowd.map(|c| c.label.as_str()).unwrap_or("town")).replace("_", " ")
            + &format!(", who lives in the {}", b.kind.replace('_', " "))),
        None => (Value::String(r.clone()), x.name.clone()),
    };
    json!({
        "ref": rf, "name": name, "resident": resident.is_some(),
        "title": x.life.as_ref().map(|l| l.rank.title.clone()).filter(|t| !t.is_empty()),
        "role": role, "look": look, "sprite": sprite_stub(&sprite_key(role, look)),
        "cell": cell, "pos": x.pos, "doing": doing, "activity": activity, "why": why, "moving": moving,
        "to_cell": to_cell, "home_cell": home_cell, "work_cell": work_cell, "work": work,
        "settlement": settlement.map(Ref::Place), "group": null,
    })
}

/// Gives people doing the same thing at the same spot (their `to_cell`, else their work or home hex, else their own
/// cell) a small shared `group` id (1, 2, … in list order); someone alone stays `null`.
pub(crate) fn assign_groups(items: &mut [Value]) {
    let spot = |v: &Value| -> String {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        let at = s("to_cell").or_else(|| match v["activity"].as_str() {
            Some("sleeping" | "resting" | "chatting" | "going_home" | "storytelling" | "brewing" | "playing") => s("home_cell"),
            _ => s("work_cell"),
        });
        format!("{}|{}", v["activity"].as_str().unwrap_or(""), at.or_else(|| s("cell")).unwrap_or_default())
    };
    let keys: Vec<String> = items.iter().map(spot).collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for k in &keys {
        *count.entry(k.as_str()).or_default() += 1;
    }
    let mut ids: HashMap<&str, u32> = HashMap::new();
    for (v, k) in items.iter_mut().zip(keys.iter()) {
        if count[k.as_str()] >= 2 {
            let n = ids.len() as u32 + 1;
            v["group"] = (*ids.entry(k.as_str()).or_insert(n)).into();
        }
    }
}

/// The people of a list, as [`person_item`]s with groups.
pub(crate) fn items(cx: &Ctx, xs: &[&Character]) -> Vec<Value> {
    let mut v: Vec<Value> = xs.iter().map(|x| person_item(cx, x)).collect();
    assign_groups(&mut v);
    v
}

// ---------------------------------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------------------------------

fn hex_distance(a: &HexCell, b: &HexCell) -> Option<i64> {
    (a.face == b.face && a.level == b.level).then(|| {
        let (dq, dr) = (a.q - b.q, a.r - b.r);
        (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
    })
}

/// The board's centre at the asked level: `cell` (any level) and `level` (default: the cell's).
fn centre(args: &Value) -> Result<HexCell, String> {
    let key = args.get("cell").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or("`cell` is required (a hex cell key)")?;
    let c = HexCell::parse(key).ok_or_else(|| format!("`{key}` is not a hex cell key (\"<face>:<level>:<q>:<r>\")"))?;
    match args.get("level").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(c),
        Some(l) => {
            let level = HexLevel::parse(l).ok_or_else(|| format!("`level` must be regions, villages, houses or people, not {l}"))?;
            Ok(if level == c.level { c } else { cell_at(c.center(), level) })
        }
    }
}

fn limit(args: &Value, def: usize, max: usize) -> Result<usize, String> {
    let n = match args.get("limit") {
        None | Some(Value::Null) => return Ok(def),
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) if s.trim().is_empty() => return Ok(def),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
        _ => None,
    };
    match n {
        Some(n) if (1..=max as u64).contains(&n) => Ok(n as usize),
        _ => Err(format!("`limit` must be 1..{max}")),
    }
}

/// `game.people {cell, level?, limit?}`: every living person in the board disc (22 rings at `level`) around `cell`,
/// nearest first, at most `limit` (default 100, at most 300), each with their sprite and what they are doing.
pub fn query_people(state: &Value, args: &Value) -> Result<Value, String> {
    let c = centre(args)?;
    let lim = limit(args, 100, MAX_PEOPLE)?;
    let w = world(state, &[T::Places, T::Groups, T::Clusters, T::Characters, T::Mobs, T::DmLite])?;
    let mut near: Vec<(i64, CharId)> = w
        .characters
        .iter()
        .filter(|x| x.alive)
        .filter_map(|x| hex_distance(&cell_at(x.pos, c.level), &c).filter(|d| *d <= RINGS).map(|d| (d, x.id)))
        .collect();
    // The residents of the settlements' houses in reach (named people first at the same distance).
    let reach_km = (RINGS as f64 + 1.0) * c.level.size_m() * 3f64.sqrt() / 1000.0 + 0.2;
    let centre_at = c.center();
    let mut res: Vec<(i64, String, Character, House)> = vec![];
    // Residents are drawn below the regions level (a continent-wide board shows the named people only).
    let in_reach: Vec<&Place> = if c.level == HexLevel::Regions {
        vec![]
    } else {
        w.places.iter().filter(|p| p.kind == PlaceKind::Site && distance_km(p.center, centre_at) <= reach_km + p.radius_km.max(0.0)).collect()
    };
    let cx = Ctx::new(&w, state);
    // Each settlement's crowds, in one pass over the crowds.
    let mut crowds_of: HashMap<PlaceId, Vec<&Cluster>> = HashMap::new();
    if !in_reach.is_empty() {
        for k in w.clusters.iter().filter(|k| k.army.is_none() && k.size > 0) {
            let mut p = Some(k.place);
            for _ in 0..8 {
                let Some(id) = p else { break };
                let Some(pl) = w.places.get(id.ix()) else { break };
                if pl.kind == PlaceKind::Site {
                    crowds_of.entry(id).or_default().push(k);
                    break;
                }
                p = pl.parent;
            }
        }
    }
    for p in in_reach {
        let crowds = crowds_of.get(&p.id).map(Vec::as_slice).unwrap_or(&[]);
        for b in cx.houses_of(p.id) {
            for (k, x) in residents(&b, crowds) {
                if let Some(d) = hex_distance(&cell_at(x.pos, c.level), &c).filter(|d| *d <= RINGS) {
                    res.push((d, k, x, b.clone()));
                }
            }
        }
    }
    near.sort();
    res.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let total = near.len() + res.len();
    // Merge by distance, at most `lim`.
    let (mut i, mut j) = (0, 0);
    let mut people: Vec<Value> = Vec::with_capacity(lim.min(total));
    while people.len() < lim && (i < near.len() || j < res.len()) {
        let named = j >= res.len() || (i < near.len() && near[i].0 <= res[j].0);
        if named {
            people.push(person_item(&cx, &w.characters[near[i].1.ix()]));
            i += 1;
        } else {
            let (_, k, x, b) = &res[j];
            people.push(item_of(&cx, x, Some((k.as_str(), b))));
            j += 1;
        }
    }
    assign_groups(&mut people);
    let moving = people.iter().filter(|p| p["moving"] == true).count();
    Ok(json!({ "cell": c.key(), "level": c.level.as_str(), "rings": RINGS, "beat": w.meta.beat, "phase": cx.phase(),
               "total": total, "named": near.len(), "residents": res.len(), "moving": moving, "people": people }))
}

/// The people living in a house: 2–4 of them, each drawn (by a stable hash of the house and their number) from the
/// crowds of its settlement, standing within a few metres of it. Synthetic and read-only: they have no `ch:` ref
/// until the story names one of them (then the society materializes a real person as always).
pub(crate) fn residents(b: &House, crowds: &[&Cluster]) -> Vec<(String, Character)> {
    if crowds.is_empty() {
        return vec![];
    }
    let key = b.cell.key();
    let n = 2 + (h(&key, 41) % 3) as usize;
    let centre = b.cell.center();
    (0..n)
        .map(|i| {
            let k = format!("{key}#{i}");
            let c = crowds[(h(&k, 42) % crowds.len() as u64) as usize];
            let pos = fn7_society::geo::destination(centre, (h(&k, 43) % 360) as f64, 0.004 + (h(&k, 44) % 9) as f64 * 0.001);
            let traits: Vec<String> = c.traits.iter().skip((h(&k, 45) % c.traits.len().max(1) as u64) as usize).take(1).cloned().collect();
            let x = Character {
                // A synthetic id above any real one, stable per resident (it seeds their role and look).
                id: CharId(0x8000_0000 | (h(&k, 46) as u32 & 0x7fff_ffff)),
                name: String::new(),
                aliases: vec![],
                origin: Origin::Materialized { cluster: c.id, index: u32::MAX },
                place: c.place,
                pos,
                traits,
                groups: c.groups.clone(),
                health: 10.0,
                max_health: 10.0,
                alive: true,
                allegiance: vec![],
                serves: None,
                possessed_by: None,
                memorized_beat: 0,
                memorized_by: None,
                tempo: vec![],
                life: None,
            };
            (k, x)
        })
        .collect()
}

/// `game.buildings {cell, level?}`: every settlement house hex in the board disc around `cell`, with its kind, look
/// and picture.
pub fn query_buildings(state: &Value, args: &Value) -> Result<Value, String> {
    let c = centre(args)?;
    let w = world(state, &[T::Places, T::DmLite])?;
    // A quick reject: the disc's reach in km (a hex is ~sqrt(3) * size across), plus the farthest a house can be from
    // its settlement's centre (two houses-level rings).
    let reach_km = (RINGS as f64 + 1.0) * c.level.size_m() * 3f64.sqrt() / 1000.0 + 0.2;
    let centre_at = c.center();
    let wrecked: std::collections::HashSet<&str> =
        w.dm.earth.as_ref().map(|e| e.wrecks.iter().map(|x| x.at.as_str()).collect()).unwrap_or_default();
    let mut out = vec![];
    for p in w.places.iter().filter(|p| p.kind == PlaceKind::Site) {
        if distance_km(p.center, centre_at) > reach_km {
            continue;
        }
        let level = w.dm.settlement(p.id).map(|s| s.level);
        for b in houses(&w, p) {
            let Some(d) = hex_distance(&cell_at(b.cell.center(), c.level), &c) else { continue };
            if d > RINGS {
                continue;
            }
            let key = b.cell.key();
            out.push((d, json!({ "cell": key, "kind": b.kind, "look": b.look, "settlement": Ref::Place(p.id),
                                 "settlement_name": p.name, "settlement_level": level, "wrecked": wrecked.contains(key.as_str()),
                                 "sprite": sprite_stub(&building_key(b.kind, b.look)) })));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1["cell"].as_str().cmp(&b.1["cell"].as_str())));
    let buildings: Vec<Value> = out.into_iter().map(|x| x.1).collect();
    Ok(json!({ "cell": c.key(), "level": c.level.as_str(), "rings": RINGS, "beat": w.meta.beat, "buildings": buildings }))
}

#[cfg(test)]
#[path = "people_tests.rs"]
mod tests;
