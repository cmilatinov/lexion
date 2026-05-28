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
fn test_invalid_cast() {
    let errors = common::assert_fails("errors/semantics/invalid_cast.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_invalid_index_base() {
    let errors = common::assert_fails("errors/semantics/invalid_index_base.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_invalid_index_type() {
    let errors = common::assert_fails("errors/semantics/invalid_index_type.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_undefined_var() {
    let errors = common::assert_fails("errors/semantics/undefined_var.lex");
    insta::assert_snapshot!(errors.join("\n"));
}
