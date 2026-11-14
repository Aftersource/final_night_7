//! [`Ref`]: a pointer to anything in the world, written as a compact string (`"ch:12"`, `"cl:3"`). The same string is used
//! in game state, prompts' hidden mapping tables, SQL projections and client payloads, so one join key spans everything.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ref {
    Demigod(DemigodId),
    Char(CharId),
    Cluster(ClusterId),
    Group(GroupId),
    God(GodId),
    Place(PlaceId),
    Item(ItemId),
    Agent(AgentId),
    Ledger(LedgerId),
    /// A cell (squad, gang, congregation, crew) of the hierarchy (schema 4).
    Cell(CellId),
    /// A mob (schema 4).
    Mob(MobId),
}

impl Ref {
    /// Something that can act, be harmed, or remember (has a mind or a body).
    pub fn is_actor(&self) -> bool {
        matches!(
            self,
            Ref::Demigod(_)
                | Ref::Char(_)
                | Ref::Cluster(_)
                | Ref::God(_)
                | Ref::Agent(_)
                | Ref::Cell(_)
                | Ref::Mob(_)
        )
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ref::Demigod(i) => i.fmt(f),
            Ref::Char(i) => i.fmt(f),
            Ref::Cluster(i) => i.fmt(f),
            Ref::Group(i) => i.fmt(f),
            Ref::God(i) => i.fmt(f),
            Ref::Place(i) => i.fmt(f),
            Ref::Item(i) => i.fmt(f),
            Ref::Agent(i) => i.fmt(f),
            Ref::Ledger(i) => i.fmt(f),
            Ref::Cell(i) => i.fmt(f),
            Ref::Mob(i) => i.fmt(f),
        }
    }
}

impl FromStr for Ref {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (p, n) = s.split_once(':').ok_or_else(|| format!("bad ref {s:?}"))?;
        let n: u32 = n.parse().map_err(|_| format!("bad ref {s:?}"))?;
        Ok(match p {
            "dg" => Ref::Demigod(DemigodId(n)),
            "ch" => Ref::Char(CharId(n)),
            "cl" => Ref::Cluster(ClusterId(n)),
            "gr" => Ref::Group(GroupId(n)),
            "gd" => Ref::God(GodId(n)),
            "pl" => Ref::Place(PlaceId(n)),
            "it" => Ref::Item(ItemId(n)),
            "ag" => Ref::Agent(AgentId(n)),
            "lg" => Ref::Ledger(LedgerId(n)),
            "ce" => Ref::Cell(CellId(n)),
            "mb" => Ref::Mob(MobId(n)),
            _ => return Err(format!("bad ref prefix {s:?}")),
        })
    }
}

impl Serialize for Ref {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Ref {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// JSON Schema (feature `schema`): a `Ref` is serialized as its prefixed string, e.g. `"ch:12"`.
#[cfg(feature = "schema")]
impl schemars::JsonSchema for Ref {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Ref".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": r"^(dg|ch|cl|gr|gd|pl|it|ag|lg|ce|mb):\d+$",
            "description": "A pointer to any row of the world: dg demigod, ch character, cl cluster, gr group, gd god, \
                            pl place, it item, ag agent, lg ledger row, ce cell, mb mob. Stable for the whole match.",
            "examples": ["ch:12"]
        })
    }
}

macro_rules! from_id {
    ($t:ty, $v:ident) => {
        impl From<$t> for Ref {
            fn from(i: $t) -> Self {
                Ref::$v(i)
            }
        }
    };
}
from_id!(DemigodId, Demigod);
from_id!(CharId, Char);
from_id!(ClusterId, Cluster);
from_id!(GroupId, Group);
from_id!(GodId, God);
from_id!(PlaceId, Place);
from_id!(ItemId, Item);
from_id!(AgentId, Agent);
from_id!(LedgerId, Ledger);
from_id!(CellId, Cell);
from_id!(MobId, Mob);

/// A sorted, deduplicated set of groups: one row's position in the society Venn diagram.
/// Intersections are linear merges; at FN7 sizes (≤ 256 groups, ≤ ~12 per member) this beats hashing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GroupSet(Vec<GroupId>);

impl GroupSet {
    pub fn new(mut v: Vec<GroupId>) -> Self {
        v.sort_unstable();
        v.dedup();
        Self(v)
    }
    pub fn as_slice(&self) -> &[GroupId] {
        &self.0
    }
    pub fn contains(&self, g: GroupId) -> bool {
        self.0.binary_search(&g).is_ok()
    }
    pub fn insert(&mut self, g: GroupId) -> bool {
        match self.0.binary_search(&g) {
            Ok(_) => false,
            Err(i) => {
                self.0.insert(i, g);
                true
            }
        }
    }
    pub fn remove(&mut self, g: GroupId) -> bool {
        match self.0.binary_search(&g) {
            Ok(i) => {
                self.0.remove(i);
                true
            }
            Err(_) => false,
        }
    }
    pub fn intersects(&self, o: &GroupSet) -> bool {
        let (mut i, mut j) = (0, 0);
        while i < self.0.len() && j < o.0.len() {
            match self.0[i].cmp(&o.0[j]) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
    pub fn intersection(&self, o: &GroupSet) -> GroupSet {
        GroupSet(self.0.iter().copied().filter(|g| o.contains(*g)).collect())
    }
    pub fn union(&self, o: &GroupSet) -> GroupSet {
        GroupSet::new(self.0.iter().chain(o.0.iter()).copied().collect())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_roundtrip() {
        for s in [
            "dg:0", "ch:12", "cl:3", "gr:255", "gd:1", "pl:9", "it:4", "ag:7", "lg:100", "ce:5", "mb:2",
        ] {
            let r: Ref = s.parse().unwrap();
            assert_eq!(r.to_string(), s);
            let j = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<Ref>(&j).unwrap(), r);
        }
        assert!("xx:1".parse::<Ref>().is_err());
    }

    #[test]
    fn venn() {
        let a = GroupSet::new(vec![GroupId(3), GroupId(1), GroupId(1), GroupId(7)]);
        let b = GroupSet::new(vec![GroupId(7), GroupId(9)]);
        assert_eq!(a.as_slice(), &[GroupId(1), GroupId(3), GroupId(7)]);
        assert!(a.intersects(&b));
        assert_eq!(a.intersection(&b).as_slice(), &[GroupId(7)]);
        assert_eq!(a.union(&b).len(), 4);
    }
}
