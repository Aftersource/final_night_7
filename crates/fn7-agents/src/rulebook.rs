//! The core of the Dungeon Master's rulebook (rules 0.8.0, docs/DM_RULEBOOK.md), as the LLM DM reads it.
//!
//! Embedded in the resolver's and the gods' system prompts only for state schema 6 and up: older recordings ask the
//! minds with the old prompts, so their LLM replay still matches request for request.

/// The core rules, concise. The full text is docs/DM_RULEBOOK.md.
pub const DM_CORE: &str = "THE DUNGEON MASTER'S CORE RULES (Final Night 7, rules 0.8.0):
1. The dice decide, you describe. Every uncertain act was rolled on a d20 before you were asked: roll + modifier against \
the target DC. crit_fail (a natural 1) = it goes wrong for the actor; fail = it falls short; success = it works; \
crit_success (a natural 20) = it works beyond hope. Never overrule a roll and never roll again.
2. The DM remembers. What the memory says about a place (a hex, a settlement, a region, the world) at an earlier beat is \
settled: an uncovered encounter stays uncovered, a grown village keeps its size, a fallen hero stays fallen, a road stays \
where it wore in. Build on it; never contradict it.
3. Places nest: hex cell, settlement, region, world. A thing decided for a settlement holds for every hex in it.
4. Settlements grow and shrink by level (1 hamlet .. 5 great city). Describe a place at its level.
5. A demigod's piece carries its story: followers, items held, wardrobe (plain, marked, adorned, regalia, mythic; ragged \
or scorched when hurt) and scars. Describe them as they are.
6. Marked hexes hide encounters (monster, treasure, trap, stranger; at night undead, ghost, thief), revealed only when \
a demigod steps on them. Loot lies on the hex where it fell until someone takes it. Shrines stand where they were built.
7. Time of day is the rules': dawn, day, dusk, night. Night is darker and stranger.
8. Damage scale 0-10, health 10. Items are never lost: holders only grow.";

/// The governing principle (rules 0.8.0, Chris's framing: history and its consequences act as lenses), concise, for the
/// LLM DM's prompts: the resolver's, the gods', the narrator's, Entropy's ramifications and the summon mind's.
pub const LENS: &str = "THE LENS AND THE ENGINE: a place's recorded history and its consequences are lenses: look through \
them before you speak; they bend what is possible there. The rules and the dice are an engine that has already \
decided: never overrule its answer, however ludicrous. Your job is to make that answer make MYSTICAL sense, through \
Rube Goldberg causality: chains of omens, rituals, absurd material requirements and coincidences that rhyme with \
earlier events (the bridge held only because the ferryman washed the keystone in platinum dust at dawn, as Memory \
demanded three beats ago).";

/// Settled facts heading (what the memory lines in a prompt are introduced with).
pub const MEMORY_HEADING: &str = "The DM's memory of this place (settled facts, oldest first; never contradict them):";
