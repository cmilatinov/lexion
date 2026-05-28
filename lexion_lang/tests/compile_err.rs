mod common;

#[test]
#[ignore = "type checker diagnostics not yet stable"]
fn test_type_mismatch() {
    let errors = common::compile("errors/semantics/type_mismatch.lex").unwrap_err();
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
#[ignore = "undefined variable diagnostics not yet implemented"]
fn test_undefined_var() {
    let errors = common::compile("errors/semantics/undefined_var.lex").unwrap_err();
    insta::assert_snapshot!(errors.join("\n"));
}
