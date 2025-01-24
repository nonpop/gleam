use crate::assert_go;

#[test]
fn fn_generics() {
    assert_go!(
        r#"pub fn identity(a) -> a {
  a
}
"#,
    );
}

#[test]
fn record_generics() {
    assert_go!(
        r#"pub type Animal(t) {
  Cat(type_: t)
  Dog(type_: t)
}

pub fn main() {
  Cat(type_: 6)
}
"#,
    );
}

#[test]
fn tuple_generics() {
    assert_go!(
        r#"pub fn make_tuple(x: t) -> #(Int, t, Int) {
  #(0, x, 1)
}
"#,
    );
}

#[test]
fn result() {
    assert_go!(
        r#"pub fn map(result, fun) {
            case result {
              Ok(a) -> Ok(fun(a))
              Error(e) -> Error(e)
            }
          }"#,
    );
}

#[test]
fn task() {
    assert_go!(
        r#"
    @external(go, "some/path", "Promise")
    pub type Promise(value)
    pub type Task(a) = fn() -> Promise(a)"#,
    );
}
