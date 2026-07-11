mod common;

#[test]
fn parser_missing_semicolon() {
    let errors = common::assert_fails("errors/parser/missing_semicolon.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn parser_rejects_sizeof_operator() {
    let errors = common::assert_fails("errors/parser/sizeof_operator.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn duplicate_local_identifier() {
    let errors = common::assert_fails("errors/semantics/duplicate_local.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

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
fn test_invalid_tuple_index() {
    let errors = common::assert_fails("errors/semantics/invalid_tuple_index.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_invalid_member() {
    let errors = common::assert_fails("errors/semantics/invalid_member.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_borrow_is_not_assignment_target() {
    let errors = common::assert_fails("errors/semantics/invalid_borrow_assignment.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_if_branch_type_mismatch() {
    let errors = common::assert_fails("errors/semantics/if_branch_type_mismatch.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_if_without_else_must_be_unit() {
    let errors = common::assert_fails("errors/semantics/if_without_else_value.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_while_condition_must_be_bool() {
    let errors = common::assert_fails("errors/semantics/while_condition_must_be_bool.lex");
    insta::assert_snapshot!(errors.join("\n"));
}

#[test]
fn test_undefined_var() {
    let errors = common::assert_fails("errors/semantics/undefined_var.lex");
    insta::assert_snapshot!(errors.join("\n"));
}
