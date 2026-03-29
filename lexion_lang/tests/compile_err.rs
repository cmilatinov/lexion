mod common;

#[test]
#[ignore = "type checker not yet fully implemented"]
fn test_type_mismatch() {
    let errors = common::compile("errors/type_mismatch.lex").unwrap_err();
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
#[ignore = "undefined variable detection not yet implemented"]
fn test_undefined_var() {
    let errors = common::compile("errors/undefined_var.lex").unwrap_err();
    insta::assert_snapshot!(errors.join("\n"));
}
