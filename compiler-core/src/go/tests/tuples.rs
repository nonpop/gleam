use crate::assert_go;

#[test]
fn tuple() {
    assert_go!(
        r#"
fn go() {
  #("1", "2", "3")
}
"#,
    );
}

#[test]
fn tuple1() {
    assert_go!(
        r#"
fn go() {
  #(
    "1111111111111111111111111111111",
    #("1111111111111111111111111111111", "2", "3"),
    "3",
  )
}
"#,
    );
}

#[test]
fn tuple_access() {
    assert_go!(
        r#"
fn go() {
  #(1, 2).0
}
"#,
    )
}

#[test]
fn tuple_with_block_element() {
    assert_go!(
        r#"
fn go() {
  #(
    "1",
    {
      "2"
      "3"
    },
  )
}
"#,
    );
}

#[test]
fn tuple_with_block_element1() {
    assert_go!(
        r#"
fn go() {
  #(
    "1111111111111111111111111111111",
    #("1111111111111111111111111111111", "2", "3"),
    "3",
  )
}
"#,
    );
}

#[test]
fn constant_tuples() {
    assert_go!(
        r#"
const a = "Hello"
const b = 1
const c = 2.0
const e = #("bob", "dug")
        "#,
    );
}

#[test]
fn constant_tuples1() {
    assert_go!(
        r#"
const e = #(
  "loooooooooooooong", "loooooooooooong", "loooooooooooooong",
  "loooooooooooooong", "loooooooooooong", "loooooooooooooong",
)
"#
    );
}

#[test]
fn case() {
    assert_go!(
        r#"
fn go(a) {
  case a {
    #(2, a) -> a
    #(1, 1) -> 1
    #(a, b) -> a + b
  }
}
"#,
    );
}

#[test]
fn nested_pattern() {
    assert_go!(
        r#"
fn go(x) {
  case x {
    #(2, #(a, b)) -> a + b
    _ -> 1
  }
}
"#,
    );
}
