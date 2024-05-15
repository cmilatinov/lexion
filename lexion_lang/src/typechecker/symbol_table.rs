use lexion_lib::petgraph::graph::NodeIndex;
use lexion_lib::tokenizer::SourceRange;

#[derive(Debug, Default)]
pub struct SymbolTableEntry {
    pub name: String,
    pub table: Option<NodeIndex>,
    pub range: Option<SourceRange>,
}

impl SymbolTableEntry {
    pub fn from_name(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    pub name: String,
    pub entries: Vec<SymbolTableEntry>,
}

impl SymbolTable {
    pub fn from_name(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }
}
