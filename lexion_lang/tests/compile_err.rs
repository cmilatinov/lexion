mod common;

#[test]
#[ignore = "type checker diagnostics not yet stable"]
fn test_type_mismatch() {
    let errors = common::assert_fails("errors/semantics/type_mismatch.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_undefined_var() {
    let errors = common::assert_fails("errors/semantics/undefined_var.lex");
    insta::assert_snapshot!(errors.join("\n"));
}
