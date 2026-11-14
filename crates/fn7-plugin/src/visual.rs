//! Character visual identity (docs/SHARED_MEDIA_SPEC.md §3.1): every demigod and named person gets a **visual sheet**
//! (build, face, outfit, two signature colors, one prop, an emblem), picked deterministically from the match seed, the
//! character's kind and id and its name: nothing that changes during a match, so the sheet is the same at every beat
//! and in every replay. Every image of the character (portrait, event, statue, army banners) reuses the sheet's
//! [`Visual::brief`], so the same person looks the same everywhere.
//!
//! Payload form: `visual: {sheet_hash, traits: {build, face, outfit, colors: [a, b], prop, emblem}}`. `sheet_hash` is
//! sha256 (hex) of the sheet version, the character's ref and its traits: a client can key art or caches on it.
//!
//! Read-only: computed from the state on the way out, never stored, so the state and replays are untouched.

use fn7_model::seed::{hash_str, Rng};
use fn7_model::*;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Bumped when the trait tables or the pick change (it is part of every `sheet_hash`).
pub const SHEET_VERSION: &str = "fn7-visual/1";

const BUILDS: &[&str] = &[
    "towering and broad-shouldered",
    "wiry and long-limbed",
    "stocky and barrel-chested",
    "lean and athletic",
    "small and quick",
    "hulking and heavy-set",
    "willowy and tall",
    "compact and muscular",
    "gaunt and angular",
    "round and sturdy",
];

const DEMIGOD_FACES: &[&str] = &[
    "square jaw and a lightning-shaped scar",
    "sharp cheekbones and glowing eyes",
    "round face and a gap-toothed grin",
    "hawk nose and heavy brows",
    "freckles and a crooked smirk",
    "shaved head with glowing runes on the temples",
    "wild beard and storm-grey eyes",
    "a cracked porcelain half-mask",
    "silver braids and a serene stare",
    "a pointed chin and mismatched eyes",
    "a halo of floating embers for hair",
    "a broken nose and a confident grin",
];

const PERSON_FACES: &[&str] = &[
    "a weathered face and kind eyes",
    "a sharp nose and suspicious squint",
    "a round face and rosy cheeks",
    "a thin mustache and a nervous smile",
    "close-cropped hair and a stern mouth",
    "a messy bun and ink-stained cheeks",
    "a bald head and bushy eyebrows",
    "a long braid and a determined look",
    "thick glasses and a wide grin",
    "a scarred lip and a steady gaze",
    "curly hair and a gap-toothed smile",
    "sunken eyes and a tired frown",
];

const DEMIGOD_OUTFITS: &[&str] = &[
    "ornate plate armor over a hoodie",
    "a tattered royal cape over street clothes",
    "a sleek racing suit with gilded trim",
    "flowing robes stitched with constellations",
    "a battered leather duster and combat boots",
    "a tracksuit under a ceremonial breastplate",
    "a high-collared coat of overlapping scales",
    "a wrestling singlet and a laurel crown",
    "a tuxedo with a torn sleeve and shin guards",
    "a patchwork mantle of stitched-together flags",
];

const PERSON_OUTFITS: &[&str] = &[
    "a work apron over rolled-up sleeves",
    "a patched wool coat and scarf",
    "a hi-vis vest and hard hat",
    "a rumpled suit and loosened tie",
    "a market vendor's smock",
    "a dented helmet and makeshift armor",
    "a hooded rain poncho",
    "a cook's whites splashed with sauce",
    "a uniform jacket with too many medals",
    "a cardigan and sensible shoes",
    "a leather jacket covered in pins",
    "overalls and rubber boots",
];

const COLORS: &[&str] = &[
    "crimson", "gold", "teal", "silver", "violet", "emerald", "tangerine", "cobalt", "magenta", "ivory", "jet black",
    "lime", "copper", "sky blue", "scarlet", "bronze",
];

const DEMIGOD_PROPS: &[&str] = &[
    "a cracked hourglass",
    "a chain of glowing keys",
    "a lantern that burns blue",
    "a war banner on a broken spear",
    "a crown worn on the wrist",
    "a deck of oversized playing cards",
    "a megaphone made of bone",
    "a sword with a dented blade",
    "a jar of captured lightning",
    "a golden abacus",
    "a skateboard etched with runes",
    "a pocket watch the size of a shield",
];

const PERSON_PROPS: &[&str] = &[
    "a wrench",
    "a battered umbrella",
    "a basket of bread",
    "a clipboard",
    "a rusty shovel",
    "a transistor radio",
    "a lantern",
    "a walking stick",
    "a satchel of letters",
    "a frying pan",
    "a bundle of protest signs",
    "a toolbox",
];

const EMBLEMS: &[&str] = &[
    "a forked lightning bolt",
    "a coiled serpent",
    "a broken crown",
    "a winged hourglass",
    "a spiral sun",
    "a stag's head",
    "a burning eye",
    "a clenched fist",
    "a cracked moon",
    "a three-headed bird",
    "a crossed key and sword",
    "a thorned rose",
    "a grinning skull",
    "an anchor wrapped in chain",
];

/// A character's visual sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Visual {
    pub sheet_hash: String,
    pub traits: Traits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Traits {
    pub build: String,
    pub face: String,
    pub outfit: String,
    /// Two signature colors.
    pub colors: [String; 2],
    pub prop: String,
    pub emblem: String,
}

impl Visual {
    pub fn to_json(&self) -> Value {
        json!(self)
    }

    /// The sheet as prompt words: "wiry and long-limbed, sharp cheekbones and glowing eyes, wearing …, in crimson and
    /// gold, carrying …, emblem: …".
    pub fn brief(&self) -> String {
        let t = &self.traits;
        format!(
            "{}, {}, wearing {} in {} and {}, carrying {}, with the emblem of {}",
            t.build, t.face, t.outfit, t.colors[0], t.colors[1], t.prop, t.emblem
        )
    }

    /// Banner words for an army of this character: "banners in crimson and gold bearing a broken crown".
    pub fn banners(&self) -> String {
        format!(
            "banners in {} and {} bearing {}",
            self.traits.colors[0], self.traits.colors[1], self.traits.emblem
        )
    }
}

fn pick(r: &mut Rng, xs: &[&str]) -> String {
    r.pick(xs).copied().unwrap_or_default().to_string()
}

fn sheet(seed: u64, tag: u64, id: u64, name: &str, rf: Ref, demigod: bool) -> Visual {
    let mut r = Rng::derive(seed, &[0x7669_7375_616c, tag, id, hash_str(name)]);
    let build = pick(&mut r, BUILDS);
    let face = pick(&mut r, if demigod { DEMIGOD_FACES } else { PERSON_FACES });
    let outfit = pick(&mut r, if demigod { DEMIGOD_OUTFITS } else { PERSON_OUTFITS });
    let a = r.below(COLORS.len() as u64) as usize;
    let b = (a + 1 + r.below(COLORS.len() as u64 - 1) as usize) % COLORS.len();
    let prop = pick(&mut r, if demigod { DEMIGOD_PROPS } else { PERSON_PROPS });
    let emblem = pick(&mut r, EMBLEMS);
    let traits = Traits {
        build,
        face,
        outfit,
        colors: [COLORS[a].to_string(), COLORS[b].to_string()],
        prop,
        emblem,
    };
    let mut h = Sha256::new();
    h.update(SHEET_VERSION.as_bytes());
    h.update(b"\n");
    h.update(rf.to_string().as_bytes());
    h.update(b"\n");
    h.update(serde_json::to_string(&traits).unwrap_or_default().as_bytes());
    Visual {
        sheet_hash: hex::encode(h.finalize()),
        traits,
    }
}

/// A demigod's sheet (seed, id, name).
pub fn demigod(w: &World, d: &Demigod) -> Visual {
    sheet(w.meta.seed, 1, d.id.0 as u64, &d.name, Ref::Demigod(d.id), true)
}

/// A named person's sheet (seed, id, name).
pub fn person(w: &World, c: &Character) -> Visual {
    sheet(w.meta.seed, 2, c.id.0 as u64, &c.name, Ref::Char(c.id), false)
}

/// The sheet of a demigod or person ref (`None` for anything else, or one the world doesn't have loaded).
pub fn of(w: &World, r: Ref) -> Option<Visual> {
    match r {
        Ref::Demigod(d) => w.demigods.get(d.ix()).map(|x| demigod(w, x)),
        Ref::Char(c) => w.characters.get(c.ix()).map(|x| person(w, x)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_are_deterministic_and_distinct() {
        let a = sheet(7, 1, 0, "Rook", Ref::Demigod(DemigodId(0)), true);
        assert_eq!(a, sheet(7, 1, 0, "Rook", Ref::Demigod(DemigodId(0)), true));
        assert_ne!(a.traits.colors[0], a.traits.colors[1]);
        assert_eq!(a.sheet_hash.len(), 64);
        // Another seed, id or name: another sheet (hash always; traits almost always).
        let others = [
            sheet(8, 1, 0, "Rook", Ref::Demigod(DemigodId(0)), true),
            sheet(7, 1, 1, "Rook", Ref::Demigod(DemigodId(1)), true),
            sheet(7, 1, 0, "Quill", Ref::Demigod(DemigodId(0)), true),
        ];
        for o in &others {
            assert_ne!(o.sheet_hash, a.sheet_hash);
        }
        assert!(others.iter().any(|o| o.traits != a.traits));
        assert!(a.brief().contains(&a.traits.emblem) && a.brief().contains(&a.traits.colors[1]));
    }
}
