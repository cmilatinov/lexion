use crate::ast::types::TypeCollection;
use crate::ast::*;
use crate::diagnostic::DiagnosticConsumer;
use crate::pipeline::PipelineStage;
use lexion_lib::miette::{NamedSource, SourceSpan};
use lexion_lib::tabled::builder::Builder;
use lexion_lib::tabled::settings::Style;
use lexion_lib::tabled::Table;
use lexion_lib::tokenizer::SpanBuilder;
use lexion_lib::Parser;
use std::sync::Arc;

#[derive(Parser)]
#[grammar(path = "lexion_lang/grammar/lexion.json")]
pub struct ParserLexion {
    pub types: TypeCollection,
}

impl Default for ParserLexion {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserLexion {
    pub fn new() -> Self {
        Self {
            types: TypeCollection::default(),
        }
    }
}

impl PipelineStage for ParserLexion {
    type Input = ();
    type Options = NamedSource<Arc<String>>;
    type Output = (Ast, TypeCollection, Table);

    fn new(_input: Self::Input) -> Self {
        Self::new()
    }

    fn exec(
        mut self,
        diag: &mut dyn DiagnosticConsumer,
        src: Self::Options,
    ) -> Option<Self::Output> {
        let mut builder = Builder::new();
        match self.parse_from_string_trace(src.inner().clone(), Some(&mut builder)) {
            Err(err) => {
                diag.error((src, err).into());
                None
            }
            Ok(ast) => {
                let mut table = builder.build();
                table.with(Style::modern());
                Some((ast, self.types, table))
            }
        }
    }
}
