//! Enzyme database types.

use serde::Deserialize;

/// A single enzyme record from comm_only_enzymes.json.
#[derive(Debug, Clone, Deserialize)]
pub struct EnzymeRecord {
    pub name: String,
    pub site: String,
    pub fst5: i64,
    pub fst3: i64,
    #[serde(default)]
    pub scd5: Option<i64>,
    #[serde(default)]
    pub scd3: Option<i64>,
    #[serde(default = "default_true")]
    pub is_palindromic: bool,
    #[serde(default)]
    pub cut_type: String,
    #[serde(default)]
    pub overhang_len: i64,
    #[serde(default)]
    pub is_cut_twice: bool,
    #[serde(default)]
    pub methylation: String,    // "sensitive" | "none"
    #[serde(default)]
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
