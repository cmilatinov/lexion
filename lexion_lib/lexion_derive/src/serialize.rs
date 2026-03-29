use serde::{Deserialize, Serialize};
use std::str::FromStr;

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

#[derive(Clone, Serialize, Deserialize)]
pub enum ParseTableAction {
    Shift(usize),
    Goto(usize),
    Reduce(usize),
    Accept,
    Reject,
    Conflict(Vec<ParseTableAction>),
}

impl FromStr for ParseTableAction {
    type Err = ();

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        if let Some(stripped) = string.strip_prefix("r") {
            let state: usize = stripped.parse().map_err(|_| {})?;
            Ok(ParseTableAction::Reduce(state))
        } else if let Some(stripped) = string.strip_prefix("s") {
            let state: usize = stripped.parse().map_err(|_| {})?;
            Ok(ParseTableAction::Shift(state))
        } else if string == "acc" {
            Ok(ParseTableAction::Accept)
        } else if string == "rej" {
            Ok(ParseTableAction::Reject)
        } else if let Ok(state) = string.parse().map_err(|_| {}) {
            Ok(ParseTableAction::Goto(state))
        } else {
            Err(())
        }
    }
}
