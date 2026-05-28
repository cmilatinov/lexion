mod common;

#[test]
fn test_type_mismatch() {
    let errors = common::assert_fails("errors/semantics/type_mismatch.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_call_arity_mismatch() {
    let errors = common::assert_fails("errors/semantics/call_arity_mismatch.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_call_argument_type_mismatch() {
    let errors = common::assert_fails("errors/semantics/call_argument_type_mismatch.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_undefined_var() {
    let errors = common::assert_fails("errors/semantics/undefined_var.lex");
    insta::assert_snapshot!(errors.join("\n"));
}
