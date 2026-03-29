use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReductionData {
    pub ty: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleData {
    pub left: String,
    pub right: Vec<String>,
    pub reduction: Option<ReductionData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseTableOverrideData {
    pub symbol: String,
    pub state: usize,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrammarData {
    pub rules: Vec<RuleData>,
    pub overrides: Option<Vec<ParseTableOverrideData>>,
}
