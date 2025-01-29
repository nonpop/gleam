use crate::assert_go;

#[test]
fn expressions() {
    assert_go!(
        r#"
fn go() {
    True
    False
    Nil
}
"#
    );
}

#[test]
fn constants() {
    assert_go!(
        r#"
const a = True
const b = False
const c = Nil
"#,
    );
}

#[test]
fn public_constants() {
    assert_go!(
        r#"
pub const a = True
pub const b = False
pub const c = Nil
"#,
    );
}

#[test]
fn operators() {
    assert_go!(
        r#"
fn go() {
    True && True
    False || False
}
"#,
    );
}

#[test]
fn assigning() {
    assert_go!(
        r#"
fn go(x, y) {
  let assert True = x
  let assert False = x
  let assert Nil = y
}
"#,
    );
}

// https://github.com/gleam-lang/gleam/issues/1112
// differentiate between prelude constructors and custom type constructors
#[test]
fn shadowed_bools_and_nil() {
    assert_go!(
        r#"
pub type True { True False Nil }
fn go(x, y) {
  let assert True = x
  let assert False = x
  let assert Nil = y
}
"#,
    );
}

#[test]
fn equality() {
    assert_go!(
        r#"
fn go(a, b) {
  a == True
  a != True
  a == False
  a != False
  a == a
  a != a
  b == Nil
  b != Nil
  b == b
}
"#,
    );
}

#[test]
fn case() {
    assert_go!(
        r#"
fn go(a) {
  case a {
    True -> 1
    False -> 0
  }
}
"#,
    );
}

#[test]
fn nil_case() {
    assert_go!(
        r#"
fn go(a) {
  case a {
    Nil -> 0
  }
}
"#,
    );
}

#[test]
fn negation() {
    assert_go!(
        "pub fn negate(x) {
    !x
}"
    );
}

#[test]
fn negation_block() {
    assert_go!(
        "pub fn negate(x) {
  !{
    123
    x
  }
}"
    );
}

#[test]
fn binop_panic_right() {
    assert_go!(
        "pub fn negate(x) {
    x && panic
}"
    );
}

#[test]
fn binop_panic_left() {
    assert_go!(
        "pub fn negate(x) {
    panic && x
}"
    );
}

#[test]
fn binop_todo_right() {
    assert_go!(
        "pub fn negate(x) {
    x && todo
}"
    );
}

#[test]
fn binop_todo_left() {
    assert_go!(
        "pub fn negate(x) {
    todo && x
}"
    );
}

#[test]
fn negate_panic() {
    assert_go!(
        "pub fn negate(x) {
  !panic
}"
    );
}

#[test]
fn negate_todo() {
    assert_go!(
        "pub fn negate(x) {
  !todo
}"
    );
}
