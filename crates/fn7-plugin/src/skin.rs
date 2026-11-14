//! A duellist's mannequin skin: `skin` on each fighter of `challenge.lock` (and so of `present.duels[]`).
//!
//! One of [`SKINS`], chosen in this order:
//! 1. a skin its player named ([`named`]: a hook, nothing sets one yet);
//! 2. gear: a held thing a god granted → `dragonscale`; a held thing made of gold → `gold` ([`from_gear`]);
//! 3. the name: FNV-1a 32-bit of the lower-cased name, mod 5 ([`by_name`]). A client can compute this one itself.
//!
//! Display only: computed from the world when the payload is built, never stored (no state schema).

use fn7_model::manifest::{GodStance, ItemState};
use fn7_model::*;

/// The skins, in hash order (index = hash mod 5).
pub const SKINS: [&str; 5] = ["wood", "obsidian", "gold", "dragonscale", "butterfly"];

/// FNV-1a, 32 bits: offset basis 2166136261, prime 16777619, over bytes.
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, b| (h ^ *b as u32).wrapping_mul(0x0100_0193))
}

/// The default skin of a name: FNV-1a 32 over the UTF-8 bytes of the name exactly as the payload's `name` has it,
/// with ASCII `A`-`Z` lower-cased (nothing else is changed: no trimming, no Unicode case folding, so every client
/// library agrees), mod 5 into [`SKINS`].
pub fn by_name(name: &str) -> &'static str {
    SKINS[(fnv1a32(name.to_ascii_lowercase().as_bytes()) % 5) as usize]
}

/// A skin's name, if it is one (for [`named`]).
pub fn parse(s: &str) -> Option<&'static str> {
    SKINS.iter().copied().find(|k| k.eq_ignore_ascii_case(s.trim()))
}

/// HOOK (not wired): a skin the player chose for this demigod, from its persona or an account setting.
///
/// Nothing carries one today: [`fn7_model::PlayerPersona`] has no such field and the accounts settings document has no
/// such key (docs/ACCOUNTS.md). When one exists (say `persona.skin`, validated on the server with [`parse`]), return it
/// here; it then wins over gear and the name's hash, and no client or payload changes.
pub fn named(_w: &World, _d: &Demigod) -> Option<&'static str> {
    None
}

fn is_gold(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|w| ["gold", "golden", "gilded", "gilt"].iter().any(|g| w.eq_ignore_ascii_case(g)))
}

/// The skin its gear gives: among the things it holds now (not armies, fleets or beasts), one a god granted (an item
/// of origin `granted`, or a manifestation an invoked god ruled `grant`) → `dragonscale`; else one whose name or
/// description says gold → `gold`.
pub fn from_gear(w: &World, d: &Demigod) -> Option<&'static str> {
    let me = Ref::Demigod(d.id);
    let mut gold = false;
    for i in w.items.iter().filter(|i| i.holders.last() == Some(&me) && i.ext.state == ItemState::Held && !i.ext.kind.is_host()) {
        let granted = matches!(i.origin, ItemOrigin::Granted { .. })
            || i.ext.manifestation.is_some_and(|m| {
                w.manifests.iter().any(|x| x.id == m && x.request.god.is_some() && x.verdict.stance == Some(GodStance::Grant))
            });
        if granted {
            return Some("dragonscale");
        }
        gold |= is_gold(&i.name) || is_gold(&i.description);
    }
    gold.then_some("gold")
}

/// The demigod's skin now.
pub fn of(w: &World, d: &Demigod) -> &'static str {
    named(w, d).or_else(|| from_gear(w, d)).unwrap_or_else(|| by_name(&d.name))
}
