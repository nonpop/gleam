use crate::{assert_go, go::tests::CURRENT_PACKAGE};

#[test]
fn bug0_had_full_path_in_pkg_selectors() {
    assert_go!(
        (
            CURRENT_PACKAGE,
            "gleam/string_tree",
            r#"
pub type StringTree
pub fn to_string(tree: StringTree) -> String { todo }
"#
        ),
        r#"
import gleam/string_tree.{type StringTree}

pub fn inspect(term: anything) -> String {
  do_inspect(term)
  |> string_tree.to_string
}

fn do_inspect(term: anything) -> StringTree { todo }
"#
    );
}

#[test]
fn bug1_tried_to_assign_variable_assignment() {
    assert_go!(
        r#"
fn inspect(x) { x }

fn debug(term) {
  term |> inspect
  term
}
"#
    );
}

#[test]
fn bug2_did_not_pass_type_args_to_id() {
    assert_go!(
        (CURRENT_PACKAGE, "other", r#"pub fn id(x) { x }"#),
        r#"
import other

fn id_id() {
  other.id(other.id)
}
"#
    );
}

#[test]
fn bug3_did_not_pass_type_args_to_phantom() {
    assert_go!(
        r#"
type Phantom(a) { Phantom }

fn phantom() { Phantom }
"#
    );
}

#[test]
fn bug4_was_unable_to_return_generic_type_from_external() {
    assert_go!(
        r#"
@external(go, "example.com/todo/gleam_stdlib", "Dict")
type Dict(k, v)

@external(go, "dict", "ToList")
fn external_to_list(dict: Dict(k, v)) -> List(#(k, v)) { todo }
"#
    );
}

#[test]
fn bug5_added_as_a_to_xy() {
    assert_go!(
        r#"
type AB { A B }
type XY { X Y }

fn foo(ab, xy) {
  case ab, xy {
    A, X -> 1
    _, _ -> 2
  }
}
"#
    );
}

#[test]
fn bug6_treated_box_field_as_private() {
    assert_go!(
        (CURRENT_PACKAGE, "box", r#"pub type Box(a) { Box(a) }"#),
        r#"
import box.{type Box, Box}
pub type BoxedString { BoxedString(box: Box(String)) }

fn unbox(x: BoxedString) {
    case x.box { Box(s) -> s }
}
"#
    );
}

#[test]
fn bug7_did_not_pass_type_args_to_box() {
    assert_go!(
        r#"
type Box(a) { Box(a) }

fn foo(x) {
    case 0 {
        _ if x == Box(0) -> 1
        _ -> 2
    }
}
"#
    );
}

#[test]
fn bug8_did_not_pass_type_args_to_box() {
    assert_go!(
        (CURRENT_PACKAGE, "box", r#"pub type Box(a) { Box(a) }"#),
        r#"
import box

fn foo(x) {
    case 0 {
        _ if x == box.Box(0) -> 1
        _ -> 2
    }
}
"#
    );
}

#[test]
fn bug9_did_not_pass_type_args_to_box() {
    assert_go!(
        r#"
type Box(a) { Box(a) }

const box = Box(1)
"#
    );
}

#[test]
fn bug10_had_incorrect_capitalization() {
    assert_go!(
        r#"
type Color { B BB }

fn foo(x) {
  case x {
    BB -> 0
    B -> 1
  }
}
"#
    );
}

#[test]
fn bug11_did_not_import_type_module() {
    assert_go!(
        (
            CURRENT_PACKAGE,
            "gleam/order",
            r#"pub type Order { Lt Eq Gt }"#
        ),
        (
            CURRENT_PACKAGE,
            "gleam/int",
            r#"
import gleam/order.{type Order}
pub fn compare(a: Int, b: Int) -> Order { todo }
"#
        ),
        r#"
import gleam/int

fn use_order_internally() {
    fn() { int.compare(1, 2) }() == fn() { int.compare(3, 4) }()
}
"#
    );
}

#[test]
fn bug12_did_not_add_head_check() {
    assert_go!(
        r#"
type Foo { Foo(bar1: Int, bar2: Int) }

fn baz1() {
  let assert [Foo(bar1: 0, ..)] = []
}

fn baz2() {
  let assert [Foo(bar2: 0, ..)] = []
}
"#
    );
}

#[test]
fn bug13_did_not_add_type_args() {
    assert_go!(
        r#"
fn id_str(x: String) { x }
fn id(x) { x }
const ids = [id_str, id]
"#
    );
}

#[test]
fn bug14_did_not_add_type_args_different_module() {
    assert_go!(
        (CURRENT_PACKAGE, "other", r#"pub fn id(x) { x }"#),
        r#"
import other
fn id_str(x: String) { x }
const ids = [id_str, other.id]
"#
    );
}

#[test]
fn bug15_did_not_add_type_args_different_module_unqualified() {
    assert_go!(
        (CURRENT_PACKAGE, "other", r#"pub fn id(x) { x }"#),
        r#"
import other.{id}
fn id_str(x: String) { x }
const ids = [id_str, id]
"#
    );
}
