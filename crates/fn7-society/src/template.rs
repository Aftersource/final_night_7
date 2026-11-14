//! The society template (`data/society_template.json`, compiled in): district, group and cluster archetypes plus name
//! lists. Setup and materialization draw from it with seeded randomness only.

use std::sync::OnceLock;

use fn7_model::GroupKind;
use serde::{Deserialize, Serialize};

pub const TEMPLATE_JSON: &str = include_str!("../../../data/society_template.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocietyTemplate {
    pub version: String,
    /// Share of the city's population represented by clusters.
    pub population_slice: f64,
    pub permeability: Permeability,
    pub districts: Vec<DistrictArchetype>,
    pub groups: Vec<GroupArchetype>,
    /// Faith name templates (`{god}`).
    pub faiths: Vec<String>,
    /// Neighborhood group name templates (`{district}`).
    pub neighborhood: Vec<String>,
    pub families: FamilyTemplate,
    /// Prefixes that keep cluster labels unique within a district.
    pub qualifiers: Vec<String>,
    pub clusters: Vec<ClusterArchetype>,
    pub names: NameLists,
}

/// Default permeability per group kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Permeability {
    pub faction: f32,
    pub trade: f32,
    pub faith: f32,
    pub neighborhood: f32,
    pub family: f32,
    pub following: f32,
    pub emergent: f32,
}

impl Permeability {
    pub fn of(&self, k: GroupKind) -> f32 {
        match k {
            GroupKind::Faction => self.faction,
            GroupKind::Trade => self.trade,
            GroupKind::Faith => self.faith,
            GroupKind::Neighborhood => self.neighborhood,
            GroupKind::Family => self.family,
            GroupKind::Following => self.following,
            GroupKind::Emergent => self.emergent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistrictArchetype {
    pub key: String,
    /// Name templates (`{Dir}` = compass word such as "North" or "Southeast").
    pub names: Vec<String>,
    pub tags: Vec<String>,
    /// Suited to the arena's center.
    #[serde(default)]
    pub central: bool,
    /// Suited to a district by the water.
    #[serde(default)]
    pub waterfront: bool,
    pub sites: Vec<String>,
    /// Roles of seeded notables ("harbormaster").
    pub notables: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupArchetype {
    pub key: String,
    pub name: String,
    pub kind: GroupKind,
    #[serde(default)]
    pub permeability: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FamilyTemplate {
    /// Inclusive range of family groups.
    pub count: [u32; 2],
    /// Name templates (`{surname}`).
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterArchetype {
    pub key: String,
    pub labels: Vec<String>,
    /// District archetype keys it prefers (empty = anywhere).
    #[serde(default)]
    pub districts: Vec<String>,
    pub traits: Vec<String>,
    pub needs: Vec<String>,
    /// Group keys every such cluster belongs to.
    #[serde(default)]
    pub groups: Vec<String>,
    /// Group keys joined with a probability.
    #[serde(default)]
    pub maybe: Vec<(String, f32)>,
    /// God slug weights for the cluster's faith.
    #[serde(default)]
    pub faith: Vec<(String, f32)>,
    /// Probability the cluster has a faith at all.
    #[serde(default = "default_faith_chance")]
    pub faith_chance: f32,
    /// Probability the cluster belongs to a family.
    #[serde(default)]
    pub family: f32,
    /// Inclusive raw size range (scaled to the city's population at setup).
    pub size: [u32; 2],
    /// Baseline `[anger, fear, hope, devotion]`.
    pub mood: [f32; 4],
}

fn default_faith_chance() -> f32 {
    0.7
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameLists {
    /// The fallback pool (places no culture claims) and the old families' surnames.
    pub given: Vec<String>,
    pub surnames: Vec<String>,
    /// Name pools by culture: people get names from the culture of the place they come from (see `crate::names`).
    #[serde(default)]
    pub cultures: Vec<NameCulture>,
    pub quirks: Vec<String>,
    pub paladin: Vec<String>,
}

/// One culture's names (single-token given names and surnames, so "Given Surname" splits cleanly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameCulture {
    pub key: String,
    /// Country names (Natural Earth) this culture names people in; checked before `subregions`.
    #[serde(default)]
    pub countries: Vec<String>,
    /// UN subregions it covers when no culture claims the country.
    #[serde(default)]
    pub subregions: Vec<String>,
    pub given: Vec<String>,
    pub surnames: Vec<String>,
}

/// The compiled-in template (parsed once).
pub fn template() -> &'static SocietyTemplate {
    static T: OnceLock<SocietyTemplate> = OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str(TEMPLATE_JSON)
            .expect("data/society_template.json is valid (checked by tests)")
    })
}
