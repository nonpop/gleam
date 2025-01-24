use std::collections::{HashMap, HashSet};

use ecow::EcoString;
use itertools::Itertools;

use crate::{
    docvec,
    go::{join, to_go_package_name, INDENT},
    pretty::{concat, line, Document, Documentable},
};

/// A collection of Go import statements from Gleam imports and from
/// external functions, to be rendered into a Go package.
///
#[derive(Debug, Default)]
pub(crate) struct Imports<'a> {
    imports: HashMap<EcoString, Import<'a>>,
}

impl<'a> Imports<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_module(
        &mut self,
        path: EcoString,
        aliases: impl IntoIterator<Item = EcoString>,
        unqualified_imports: impl IntoIterator<Item = Member<'a>>,
    ) {
        let import = self
            .imports
            .entry(path.clone())
            .or_insert_with(|| Import::new(path.clone()));
        import
            .aliases
            .extend(aliases.into_iter().map(|alias| to_go_package_name(&alias)));
        import.unqualified.extend(unqualified_imports)
    }

    pub fn into_doc(self) -> Document<'a> {
        docvec![
            "import (",
            concat(
                self.imports
                    .values()
                    .sorted_by(|a, b| a.path.cmp(&b.path))
                    .map(|import| docvec![line(), Import::into_doc((*import).clone())]),
            )
            .nest(INDENT),
            line(),
            ")",
            line(),
            line(),
            concat(
                self.imports
                    .into_values()
                    .sorted_by(|a, b| a.path.cmp(&b.path))
                    .map(
                        |import| concat(import.aliases.iter().sorted().map(|alias| docvec![
                            "const _ = ",
                            alias,
                            ".Use_Import",
                            line()
                        ]))
                    )
            )
        ]
    }

    pub fn is_empty(&self) -> bool {
        self.imports.is_empty()
    }
}

#[derive(Debug, Clone)]
struct Import<'a> {
    path: EcoString,
    aliases: HashSet<EcoString>,
    unqualified: Vec<Member<'a>>,
}

impl<'a> Import<'a> {
    fn new(path: EcoString) -> Self {
        Self {
            path,
            aliases: Default::default(),
            unqualified: Default::default(),
        }
    }

    pub fn into_doc(self) -> Document<'a> {
        if self.aliases.is_empty() {
            docvec!["\"", self.path, "\""]
        } else {
            join(
                self.aliases
                    .into_iter()
                    .sorted()
                    .map(|alias| docvec![alias, " \"", self.path.clone(), "\"",]),
                line(),
            )
        }
    }
}

#[derive(Debug, Clone)]
pub struct Member<'a> {
    pub name: Document<'a>,
    pub alias: Option<Document<'a>>,
}

impl<'a> Member<'a> {
    fn into_doc(self) -> Document<'a> {
        match self.alias {
            None => self.name,
            Some(alias) => docvec![self.name, " as ", alias],
        }
    }
}

#[test]
fn into_doc() {
    let mut imports = Imports::new();
    imports.register_module("./gleam/empty".into(), [], []);
    imports.register_module(
        "./multiple/times".into(),
        ["wibble".into(), "wobble".into()],
        [],
    );
    imports.register_module("./multiple/times".into(), ["wubble".into()], []);
    imports.register_module(
        "./multiple/times".into(),
        [],
        [Member {
            name: "one".to_doc(),
            alias: None,
        }],
    );

    imports.register_module(
        "./other".into(),
        [],
        [
            Member {
                name: "one".to_doc(),
                alias: None,
            },
            Member {
                name: "one".to_doc(),
                alias: Some("onee".to_doc()),
            },
            Member {
                name: "two".to_doc(),
                alias: Some("twoo".to_doc()),
            },
        ],
    );

    imports.register_module(
        "./other".into(),
        [],
        [
            Member {
                name: "three".to_doc(),
                alias: None,
            },
            Member {
                name: "four".to_doc(),
                alias: None,
            },
        ],
    );

    imports.register_module(
        "./zzz".into(),
        [],
        [
            Member {
                name: "one".to_doc(),
                alias: None,
            },
            Member {
                name: "two".to_doc(),
                alias: None,
            },
        ],
    );

    assert_eq!(
        line().append(imports.into_doc()).to_pretty_string(40),
        r#"
import (
  "./gleam/empty"
  wibble_P "./multiple/times"
  wobble_P "./multiple/times"
  wubble_P "./multiple/times"
  "./other"
  "./zzz"
)

const _ = wibble_P.Use_Import
const _ = wobble_P.Use_Import
const _ = wubble_P.Use_Import
"#
        .to_string()
    );
}
