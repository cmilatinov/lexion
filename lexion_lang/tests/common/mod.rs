use enumflags2::BitFlag;
use lexion_lang::compiler::{LexionCompiler, LexionCompilerOptions};
use lexion_lang::{Dump, DumpFlags};
use lexion_lib::miette::NamedSource;
use std::sync::Arc;

pub fn compile(fixture: &str) -> Result<(), Vec<String>> {
    let path = format!("tests/fixtures/{fixture}");
    let source_code = Arc::new(std::fs::read_to_string(&path).expect("fixture not found"));
    let source = NamedSource::new(&path, source_code);
    let options = LexionCompilerOptions {
        dump_flags: DumpFlags::from(Dump::all()),
        dump_dir: "target/test-dumps".into(),
    };
    LexionCompiler::new(options)
        .exec(source)
        .map(|_| ())
        .map_err(|diag| diag.list.iter().map(|d| d.to_string()).collect())
}
