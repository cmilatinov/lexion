use std::collections::HashMap;

use generational_arena::Index;

use crate::ast::{FunctionType, TypeCollection};
use crate::diagnostic::LexionDiagnosticError;

pub type OperatorRule =
    dyn Fn(&mut TypeCollection, &[Index]) -> Result<Vec<FunctionType>, LexionDiagnosticError>;

#[derive(Default)]
pub struct OperatorTable {
    definitions: HashMap<String, Vec<FunctionType>>,
    rules: HashMap<String, Vec<Box<OperatorRule>>>,
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

    pub fn add_rule(&mut self, operator: String, rule: Box<OperatorRule>) {
        self.rules.entry(operator).or_default().push(rule);
    }

    pub fn add_rule_multiple<F: Fn() -> Box<OperatorRule>>(&mut self, operators: &[&str], rule: F) {
        for operator in operators {
            self.add_rule((*operator).into(), rule());
        }
    }

    pub fn candidate_definitions(
        &self,
        operator: &str,
        type_list: &[Index],
        types: &mut TypeCollection,
    ) -> Result<Vec<FunctionType>, LexionDiagnosticError> {
        let mut result = Vec::new();
        if let Some(tys) = self.definitions.get(operator) {
            result.extend(
                tys.iter()
                    .filter(|t| t.params.len() == 1 || t.params.as_slice().eq(type_list))
                    .cloned(),
            );
        }
        if let Some(rules) = self.rules.get(operator) {
            let rule_defs = rules
                .iter()
                .map(|r| r(types, type_list))
                .collect::<Vec<_>>();
            for res in rule_defs {
                match res {
                    Ok(defs) => {
                        result.extend(defs.into_iter());
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }
        }
        Ok(result)
    }
}
