use crate::assert_go;

#[test]
fn type_alias() {
    assert_go!(
        r#"
pub type Headers = List(#(String, String))
"#,
    );
}

#[test]
fn private_type_in_opaque_type() {
    assert_go!(
        r#"
type PrivateType {
  PrivateType
}

pub opaque type OpaqueType {
  OpaqueType(PrivateType)
}
"#,
    );
}

#[test]
fn import_indirect_type_alias() {
    assert_go!(
        (
            "wibble",
            "wibble",
            r#"
pub type Wibble {
  Wibble(Int)
}
"#
        ),
        (
            "wobble",
            "wobble",
            r#"
import wibble
pub type Wobble = wibble.Wibble
"#
        ),
        r#"
import wobble

pub fn main(x: wobble.Wobble) {
  Nil
}
"#,
    );
}
