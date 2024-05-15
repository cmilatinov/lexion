use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reduction {
    pub ty: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub left: String,
    pub right: Vec<String>,
    pub reduction: Option<Reduction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grammar {
    pub definitions: String,
    pub rules: Vec<Rule>,
}
