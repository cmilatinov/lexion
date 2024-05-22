use std::collections::HashMap;

use generational_arena::Index;

use crate::ast::FunctionType;

#[derive(Default)]
pub struct OperatorTable {
    definitions: HashMap<String, Vec<FunctionType>>,
}

impl OperatorTable {
    pub fn add_definition_multiple(
        &mut self,
        operators: &[&str],
        definitions: &[FunctionType],
    ) -> bool {
        operators.iter().all(|op| {
            definitions
                .iter()
                .all(|d| self.add_definition(String::from(*op), d.clone()))
        })
    }

    pub fn add_definition(&mut self, operator: String, definition: FunctionType) -> bool {
        if self
            .definitions
            .entry(operator.clone())
            .or_default()
            .iter()
            .find(|d| definition.eq(d))
            .is_some()
        {
            return false;
        }
        self.definitions
            .entry(operator)
            .or_default()
            .push(definition);
        true
    }

    pub fn candidate_definitions(&self, operator: &str, type_list: &[Index]) -> Vec<FunctionType> {
        if let Some(tys) = self.definitions.get(operator) {
            return tys
                .iter()
                .filter(|t| {
                    t.params.len() == type_list.len()
                        && (t.params.len() == 1
                            || t.params.iter().enumerate().any(|(i, p)| *p == type_list[i]))
                })
                .cloned()
                .collect();
        }
        Vec::new()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<FunctionType>)> {
        self.definitions.iter()
    }
}
