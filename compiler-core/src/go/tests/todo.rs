use crate::assert_go;

#[test]
fn without_message() {
    assert_go!(
        r#"
fn go() {
    todo
}
"#,
    );
}

#[test]
fn with_message() {
    assert_go!(
        r#"
fn go() {
  todo as "I should do this"
}
"#,
    );
}

#[test]
fn with_message_expr() {
    assert_go!(
        r#"
fn go() {
  let x = "I should " <> "do this"
  todo as x
}
"#,
    );
}

// https://github.com/gleam-lang/gleam/issues/1238
#[test]
fn as_expression() {
    assert_go!(
        r#"
fn go(f) {
  let boop = todo as "I should do this"
  f(todo as "Boom")
}
"#,
    );
}
