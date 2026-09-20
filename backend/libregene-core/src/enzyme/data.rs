//! Enzyme database types.

use serde::{Deserialize, Serialize};

/// A single enzyme record from comm_only_enzymes.json.
///
/// The JSON source uses snake_case keys; serde aliases keep that working for
/// parsing while the wire format to the frontend stays camelCase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnzymeRecord {
    pub name: String,
    pub site: String,
    pub fst5: i64,
    pub fst3: i64,
    #[serde(default)]
    pub scd5: Option<i64>,
    #[serde(default)]
    pub scd3: Option<i64>,
    #[serde(default = "default_true", alias = "is_palindromic")]
    pub is_palindromic: bool,
    #[serde(default, alias = "cut_type")]
    pub cut_type: String,
    #[serde(default, alias = "overhang_len")]
    pub overhang_len: i64,
    #[serde(default, alias = "is_cut_twice")]
    pub is_cut_twice: bool,
    #[serde(default)]
    pub methylation: String,    // "sensitive" | "none"
    #[serde(default, alias = "methylation_dependent")]
    pub methylation_dependent: bool,  // requires methylation to cut (e.g. DpnI)
    #[serde(default)]
    pub elucidate: String,
}

impl EnzymeRecord {
    /// True if this enzyme is blocked by methylation (methylation-sensitive).
    pub fn is_methylation_sensitive(&self) -> bool {
        self.methylation == "sensitive"
    }
}

fn default_true() -> bool {
    true
}

/// In-memory enzyme database loaded from comm_only_enzymes.json.
#[derive(Debug, Clone)]
pub struct EnzymeDb {
    pub enzymes: Vec<EnzymeRecord>,
}

/// Buffer compatibility entry from enzyme_providers.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferActivity {
    pub name: String,
    #[serde(default)]
    pub activity: String,
}

/// One supplier's product data for an enzyme. All fields optional in the
/// source JSON (e.g. some entries lack a recommended buffer or catalog no.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    #[serde(default)]
    pub recommended_buffer: String,
    #[serde(default)]
    pub buffers: Vec<BufferActivity>,
    #[serde(default)]
    pub work_temp: String,
    #[serde(default)]
    pub heat_inactivation: String,
    #[serde(default)]
    pub methylation: String,
    #[serde(default)]
    pub star_activity: String,
    #[serde(default)]
    pub catalog: String,
}

/// Provider data for a main-DB enzyme, keyed by DB enzyme name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnzymeProviderEntry {
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderInfo>,
}

/// Enzyme that only exists in supplier catalogs (nicking/homing enzymes
/// absent from the main DB).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOnlyEnzyme {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub providers: std::collections::HashMap<String, ProviderInfo>,
}

/// In-memory supplier database loaded from enzyme_providers.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderData {
    pub enzymes: std::collections::HashMap<String, EnzymeProviderEntry>,
    #[serde(default)]
    pub provider_only: Vec<ProviderOnlyEnzyme>,
}
