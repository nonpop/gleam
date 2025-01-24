use crate::{assert_go, assert_go_error, assert_module_error};

#[test]
fn type_without_attribute() {
    assert_go!(r#"pub type Thing"#,);
}

#[test]
fn type_with_attribute() {
    assert_go!(
        r#"
@external(go, "some/path", "SomeType")
pub type Thing
"#
    );
}

#[test]
fn type_with_body_and_external_go() {
    assert_go!(
        r#"
@external(go, "some/path", "SomeType")
pub type Thing { Thing }
"#
    );
}

#[test]
fn type_with_body_and_external_other() {
    assert_go!(
        r#"
@external(javascript, "some/path", "SomeType")
pub type Thing { Thing }
"#
    );
}

#[test]
fn generic_type_with_attribute() {
    assert_go!(
        r#"
@external(go, "some/path", "SomeType")
pub type Thing(a, b)
"#
    );
}

#[test]
fn external_type_in_current_go_package() {
    assert_go!(
        r#"
@external(go, "", "thing_t")
pub type Thing(a, b)
"#
    );
}

#[test]
fn external_type_in_current_go_package_same_name() {
    assert_go!(
        r#"
@external(go, "", "Thing_t")
pub type Thing(a, b)
"#
    );
}

#[test]
fn module_fn() {
    assert_go!(
        r#"
@external(go, "utils", "Inspect")
fn show(x: anything) -> Nil"#,
    );
}

#[test]
fn full_package_name() {
    assert_go!(
        r#"
@external(go, "example.com/organization/module", "Inspect")
fn show(x: anything) -> Nil"#,
    );
}

#[test]
fn full_package_name_with_alias() {
    assert_go!(
        r#"
@external(go, "module example.com/organization/module/v2", "Inspect")
fn show(x: anything) -> Nil"#,
    );
}

#[test]
fn empty_package_same_name() {
    assert_go!(
        r#"
@external(go, "", "Inspect")
pub fn inspect(x: anything) -> Nil"#,
    );
}

#[test]
fn empty_package_different_name() {
    assert_go!(
        r#"
@external(go, "", "inspect")
pub fn inspect(x: anything) -> Nil"#,
    );
}

#[test]
fn pub_module_fn() {
    assert_go!(
        r#"
@external(go, "utils", "Inspect")
pub fn show(x: anything) -> Nil"#,
    );
}

#[test]
fn same_name_external() {
    assert_go!(
        r#"
@external(go, "thingy", "Fetch")
pub fn fetch(request: Nil) -> Nil"#,
    );
}

#[test]
fn same_module_multiple_imports() {
    assert_go!(
        r#"
@external(go, "the/module", "One")
pub fn one() -> Nil

@external(go, "the/module", "Two")
pub fn two() -> Nil
"#,
    );
}

#[test]
fn duplicate_import() {
    assert_go!(
        r#"
@external(go, "the/module", "Dup")
pub fn one() -> Nil

@external(go, "the/module", "Dup")
pub fn two() -> Nil
"#,
    );
}

#[test]
fn name_to_escape() {
    assert_go!(
        r#"
@external(go, "the/module", "One")
fn func() -> Nil
"#,
    );
}

#[test]
fn external_type() {
    assert_go!(
        r#"
@external(go, "queue", "Queue")
pub type Queue(a)

@external(go, "queue", "New")
pub fn new() -> Queue(a)
"#,
    );
}

// https://github.com/gleam-lang/gleam/issues/1636
#[test]
fn external_fn_escaping() {
    assert_go!(
        r#"
@external(go, "var", "Then")
pub fn then(a: a) -> b"#,
    );
}

// https://github.com/gleam-lang/gleam/issues/1954
#[test]
fn pipe_variable_shadow() {
    assert_go!(
        r#"
@external(go, "module", "String")
fn name() -> String

pub fn main() {
  let name = name()
  name
}
"#
    );
}

// https://github.com/gleam-lang/gleam/issues/2090
#[test]
fn tf_type_name_usage() {
    assert_go!(
        r#"
@external(go, "tf", "TF")
pub type TESTitem

@external(go, "it", "One")
pub fn one(a: TESTitem) -> TESTitem
"#
    );
}

#[test]
fn attribute_erlang() {
    assert_go!(
        r#"
@external(erlang, "one", "one_erl")
pub fn one(x: Int) -> Int {
  todo
}

pub fn main() {
  one(1)
}
"#
    );
}

#[test]
fn attribute_go() {
    assert_go!(
        r#"
@external(go, "one", "OneGo")
pub fn one(x: Int) -> Int {
  todo
}

pub fn main() {
  one(1)
}
"#
    );
}

#[test]
fn erlang_and_go() {
    assert_go!(
        r#"
@external(erlang, "one", "one")
@external(go, "one", "OneGo")
pub fn one(x: Int) -> Int {
  todo
}

pub fn main() {
  one(1)
}
"#
    );
}

#[test]
fn private_attribute_erlang() {
    assert_go!(
        r#"
@external(erlang, "one", "one_erl")
fn one(x: Int) -> Int {
  todo
}

pub fn main() {
  one(1)
}
"#
    );
}

#[test]
fn private_attribute_go() {
    assert_go!(
        r#"
@external(go, "one", "OneGo")
fn one(x: Int) -> Int {
  todo
}

pub fn main() {
  one(1)
}
"#
    );
}

#[test]
fn private_erlang_and_go() {
    assert_go!(
        r#"
@external(erlang, "one", "one")
@external(go, "one", "OneGo")
fn one(x: Int) -> Int {
  todo
}

pub fn main() {
  one(1)
}
"#
    );
}

#[test]
fn no_body() {
    assert_go!(
        r#"
@external(go, "one", "One")
pub fn one(x: Int) -> Int
"#
    );
}

#[test]
fn no_module() {
    assert_go!(
        r#"
@external(go, "", "One")
pub fn one(x: Int) -> Int {
  1
}
"#
    );
}

#[test]
fn inline_function() {
    assert_module_error!(
        r#"
@external(go, "blah", "func(x int) { return x}")
pub fn one(x: Int) -> Int {
  1
}
"#
    );
}

#[test]
fn erlang_only() {
    assert_go!(
        r#"
pub fn should_be_generated(x: Int) -> Int {
  x
}

@external(erlang, "one", "one")
pub fn should_not_be_generated(x: Int) -> Int
"#
    );
}

#[test]
fn erlang_bit_patterns() {
    assert_go_error!(
        r#"
pub fn should_not_be_generated(x) {
  case x {
    <<_, rest:bits>> -> rest
    _ -> x
  }
}
"#
    );
}

#[test]
fn both_externals_no_valid_impl() {
    assert_go!(
        r#"
@external(go, "one", "One")
pub fn go() -> Nil

@external(erlang, "one", "one")
pub fn erl() -> Nil

pub fn should_not_be_generated() {
  go()
  erl()
}
"#
    );
}

#[test]
fn discarded_names_in_external_are_passed_correctly() {
    assert_go!(
        r#"
@external(go, "wibble", "Wobble")
pub fn woo(_ignored: a) -> Nil
"#
    );
}
