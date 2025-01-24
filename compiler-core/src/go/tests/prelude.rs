use crate::assert_go;

#[test]
fn qualified_ok() {
    assert_go!(
        r#"import gleam
pub fn go() { gleam.Ok(1) }
"#,
    );
}

#[test]
fn qualified_error() {
    assert_go!(
        r#"import gleam
pub fn go() { gleam.Error(1) }
"#,
    );
}

#[test]
fn qualified_nil() {
    assert_go!(
        r#"import gleam
pub fn go() { gleam.Nil }
"#,
    );
}
