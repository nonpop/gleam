mod endianness;
mod expression;
mod import;
mod pattern;
#[cfg(test)]
mod tests;

use std::ops::Deref;
use std::sync::Arc;

use expression::comma_separated_list;
use heck::{ToLowerCamelCase, ToUpperCamelCase};
use im::HashSet;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::analyse::{get_compatible_record_fields, TargetSupport};
use crate::build::Target;
use crate::type_::{is_prelude_module, ModuleInterface, Type, TypeVar, PRELUDE_MODULE_NAME};
use crate::{
    ast::{CustomType, Function, Import, ModuleConstant, TypeAlias, *},
    docvec,
    line_numbers::LineNumbers,
    pretty::*,
};
use camino::Utf8Path;
use ecow::{eco_format, EcoString};
use itertools::Itertools;

use self::import::{Imports, Member};

const INDENT: isize = 2; // TODO: tabs

pub const PRELUDE: &str = include_str!("../templates/prelude.go");

pub type Output<'a> = Result<Document<'a>, Error>;

#[derive(Debug)]
pub struct Generator<'a> {
    dep_modules: &'a im::HashMap<EcoString, ModuleInterface>,
    line_numbers: &'a LineNumbers,
    module: &'a TypedModule,
    tracker: UsageTracker,
    module_scope: im::HashMap<EcoString, (bool, usize)>,
    target_support: TargetSupport,
    go_module_path: &'a str,
}

enum DynamicSpecialCases {
    Option,
}

impl<'a> Generator<'a> {
    pub fn new(
        dep_modules: &'a im::HashMap<EcoString, ModuleInterface>,
        line_numbers: &'a LineNumbers,
        module: &'a TypedModule,
        target_support: TargetSupport,
        go_module_path: &'a str,
    ) -> Self {
        Self {
            dep_modules,
            line_numbers,
            module,
            tracker: UsageTracker::default(),
            module_scope: Default::default(),
            target_support,
            go_module_path,
        }
    }

    pub fn compile(&mut self) -> Output<'a> {
        let package_short_name = self
            .module
            .name
            .split('/')
            .last()
            .expect("failed to get short package name");
        let package_decl: EcoString =
            format!("package {}\n", to_go_package_name(package_short_name)).into();

        // Determine what Go imports we need to generate
        let mut imports = self.collect_imports();

        // Determine what names are defined in the module scope so we know to
        // rename any variables that are defined within functions using the same
        // names.
        self.register_module_definitions_in_scope();

        // Generate Go code for each statement
        let statements = self.collect_definitions().into_iter().chain(
            self.module
                .definitions
                .iter()
                .flat_map(|s| self.statement(s)),
        );

        // Two lines between each statement
        let statements: Vec<_> = Itertools::intersperse(statements, Ok(lines(2))).try_collect()?;

        // Import any Go stdlib functions that have been used
        if self.tracker.go_fmt_used {
            self.register_go_usage(&mut imports, "fmt");
        }
        if self.tracker.go_strings_used {
            self.register_go_usage(&mut imports, "strings");
        }

        // Import any prelude functions that have been used

        self.register_prelude_usage(&mut imports, "", None);

        if self.tracker.tuple_used {
            self.register_prelude_usage(&mut imports, "Tuple", None);
        };

        if self.tracker.nil_used {
            self.register_prelude_usage(&mut imports, "Nil", None);
        };

        if self.tracker.int_used {
            self.register_prelude_usage(&mut imports, "Int", None);
        }
        if self.tracker.float_used {
            self.register_prelude_usage(&mut imports, "Float", None);
        }
        if self.tracker.string_used {
            self.register_prelude_usage(&mut imports, "String", None);
        }
        if self.tracker.bool_used {
            self.register_prelude_usage(&mut imports, "Bool", None);
        }

        if self.tracker.result_used {
            self.register_prelude_usage(&mut imports, "Result", None);
        };

        if self.tracker.ok_used {
            self.register_prelude_usage(&mut imports, "Ok", None);
        };

        if self.tracker.error_used {
            self.register_prelude_usage(&mut imports, "Error", None);
        };

        if self.tracker.list_used {
            self.register_prelude_usage(&mut imports, "toList", None);
        };

        if self.tracker.prepend_used {
            self.register_prelude_usage(&mut imports, "prepend", Some("listPrepend"));
        };

        if self.tracker.custom_type_used {
            self.register_prelude_usage(&mut imports, "CustomType", Some("$CustomType"));
        };

        if self.tracker.make_error_used {
            self.register_prelude_usage(&mut imports, "makeError", None);
        };

        if self.tracker.int_remainder_used {
            self.register_prelude_usage(&mut imports, "remainderInt", None);
        };

        if self.tracker.float_division_used {
            self.register_prelude_usage(&mut imports, "divideFloat", None);
        };

        if self.tracker.int_division_used {
            self.register_prelude_usage(&mut imports, "divideInt", None);
        };

        if self.tracker.object_equality_used {
            self.register_prelude_usage(&mut imports, "isEqual", None);
        };

        if self.tracker.bit_array_used {
            self.register_prelude_usage(&mut imports, "BitArray", None);
        };

        if self.tracker.utf_codepoint_used {
            self.register_prelude_usage(&mut imports, "UtfCodepoint", None);
        };

        if self.tracker.bit_array_literal_used {
            self.register_prelude_usage(&mut imports, "toBitArray", None);
        };

        if self.tracker.sized_integer_segment_used {
            self.register_prelude_usage(&mut imports, "sizedInt", None);
        };

        if self.tracker.string_bit_array_segment_used {
            self.register_prelude_usage(&mut imports, "stringBits", None);
        };

        if self.tracker.codepoint_bit_array_segment_used {
            self.register_prelude_usage(&mut imports, "codepointBits", None);
        };

        if self.tracker.float_bit_array_segment_used {
            self.register_prelude_usage(&mut imports, "sizedFloat", None);
        };

        let use_imports = "const Use_Import byte = 0".to_doc();

        // Put it all together

        if imports.is_empty() && statements.is_empty() {
            Ok(docvec![package_decl, line(), use_imports, line(), line()])
        } else if imports.is_empty() {
            Ok(docvec![
                package_decl,
                line(),
                use_imports,
                line(),
                line(),
                statements,
                line()
            ])
        } else if statements.is_empty() {
            Ok(docvec![
                package_decl,
                line(),
                imports.into_doc(),
                use_imports,
                line(),
                line(),
            ])
        } else {
            Ok(docvec![
                package_decl,
                line(),
                imports.into_doc(),
                use_imports,
                line(),
                line(),
                statements,
                line()
            ])
        }
    }

    fn register_prelude_usage(
        &self,
        imports: &mut Imports<'a>,
        _name: &'static str,
        _alias: Option<&'static str>,
    ) {
        let path = self.import_path("", PRELUDE_MODULE_NAME);
        imports.register_module(path, [PRELUDE_MODULE_NAME.into()], []);
    }

    fn register_go_usage(&self, imports: &mut Imports<'a>, name: &'static str) {
        imports.register_module(EcoString::from(name), [], []);
    }

    pub fn statement(&mut self, statement: &'a TypedDefinition) -> Option<Output<'a>> {
        match statement {
            Definition::TypeAlias(TypeAlias { .. }) => None,

            // Handled in collect_imports
            Definition::Import(Import { .. }) => None,

            // Handled in collect_definitions
            Definition::CustomType(CustomType { .. }) => None,

            Definition::ModuleConstant(ModuleConstant {
                publicity,
                name,
                value,
                ..
            }) => Some(self.module_constant(*publicity, name, value)),

            Definition::Function(function) => {
                // If the function does not support Go then we don't need to generate
                // a function definition.
                if !function.implementations.supports(Target::Go) {
                    return None;
                }

                self.module_function(function)
            }
        }
    }

    fn custom_type_definition(
        &mut self,
        name: &str,
        publicity: Publicity,
        constructors: &'a [TypedRecordConstructor],
        opaque: bool,
        _parameters: &Vec<EcoString>,
        typed_parameters: &[Arc<Type>],
        external_go: Option<(EcoString, EcoString)>,
    ) -> Vec<Output<'a>> {
        let has_type_params = !typed_parameters.is_empty();
        let generic_ids = collect_generic_usages(HashSet::new(), typed_parameters);
        let type_params = generic_ids.iter().sorted().map(|id| id_to_type_var(*id));

        let type_params_full_doc = if has_type_params {
            wrap_generic_params(type_params.clone())
        } else {
            nil()
        };

        let type_params_paren = if has_type_params {
            break_("", "")
                .append(join(type_params.clone(), break_(",", ", ")))
                .nest(INDENT)
                .append(break_("", ""))
                .surround("(", ")")
                .group()
        } else {
            "()".to_doc()
        };

        let type_params_sqparen = if has_type_params {
            wrap_generic_args(type_params.clone())
        } else {
            nil()
        };

        let type_name = to_go_type_name(name, publicity.is_public());
        let cons_public = publicity.is_public() && !opaque;

        if constructors.len() == 1 {
            match &external_go {
                Some((go_pkg, go_name)) if go_pkg == "" && go_name == &type_name => vec![],
                Some((go_pkg, go_name)) => vec![Ok(docvec![
                    "type ",
                    &type_name,
                    type_params_full_doc.clone(),
                    " = ",
                    if go_pkg != "" {
                        docvec![to_go_package_name(go_pkg), "."]
                    } else {
                        nil()
                    },
                    go_name,
                    type_params_sqparen.clone()
                ])],
                None => {
                    let con = &constructors[0];
                    let con_name = to_go_constructor_name(&con.name, cons_public);

                    let con_def = docvec![
                        "type ",
                        &con_name,
                        type_params_full_doc.clone(),
                        " struct ",
                        if con.arguments.len() == 0 {
                            "{}".to_doc()
                        } else {
                            wrap_curly_semi(con.arguments.iter().enumerate().map(|(i, arg)| {
                                docvec![
                                    arg.label
                                        .as_ref()
                                        .map(|(_, s)| to_go_common_field_name(
                                            s,
                                            cons_public,
                                            true,
                                            false
                                        ))
                                        .unwrap_or(to_go_positional_field_name(
                                            i.try_into().unwrap(),
                                            cons_public
                                        )),
                                    " ",
                                    type_doc(
                                        &self.module,
                                        &arg.type_,
                                        &mut self.tracker,
                                        &HashSet::new()
                                    ),
                                ]
                            }))
                        }
                    ];

                    let con_hash = docvec![
                        "func (c ",
                        &con_name,
                        type_params_sqparen.clone(),
                        ") Hash() uint32 {",
                        if con.arguments.is_empty() {
                            docvec![
                                "return ",
                                to_go_package_name(PRELUDE_MODULE_NAME),
                                ".NilHash }"
                            ]
                        } else {
                            docvec![
                                docvec![
                                    line(),
                                    "h := ",
                                    to_go_package_name(PRELUDE_MODULE_NAME),
                                    ".NewHash()",
                                    line(),
                                    "var hh uint32",
                                    con.arguments.iter().enumerate().map(
                                        |(i, arg)| {
                                            docvec![
                                                line(),
                                                "hh = c.",
                                                arg.label
                                                    .as_ref()
                                                    .map(|(_, s)| to_go_common_field_name(
                                                        s,
                                                        cons_public,
                                                        true,
                                                        false
                                                    ))
                                                    .unwrap_or(to_go_positional_field_name(
                                                        i.try_into().unwrap(),
                                                        cons_public
                                                    )),
                                                ".Hash()",
                                                line(),
                                                "if _, err := h.Write([]byte{byte(hh), byte(hh >> 8), byte(hh >> 16), byte(hh >> 24)}); err != nil { panic (err) }"
                                            ]
                                        }
                                    )
                                    .collect::<Vec<_>>(),
                                    line(),
                                    "return h.Sum32()",
                                ]
                                .nest(INDENT),
                                line(),
                                "}",
                            ]
                        },
                        line(),
                        "func (c ",
                        &con_name,
                        type_params_sqparen.clone(),
                        ") Equal(o ",
                        &con_name,
                        type_params_sqparen.clone(),
                        ") bool {",
                        docvec![
                            line(),
                            "_ = o",
                            con.arguments
                                .iter()
                                .enumerate()
                                .map(|(arg_idx, arg)| {
                                    let lbl = arg
                                        .label
                                        .as_ref()
                                        .map(|(_, s)| to_go_field_name(s, cons_public))
                                        .unwrap_or(to_go_positional_field_name(
                                            arg_idx.try_into().unwrap(),
                                            cons_public,
                                        ));
                                    docvec![
                                        line(),
                                        "if !c.",
                                        &lbl,
                                        ".Equal(o.",
                                        &lbl,
                                        ") { return false }"
                                    ]
                                })
                                .collect::<Vec<_>>(),
                            line(),
                            "return true",
                        ]
                        .nest(INDENT),
                        line(),
                        "}",
                    ];

                    let type_def = docvec![
                        "type ",
                        &type_name,
                        type_params_full_doc.clone(),
                        " = ",
                        &con_name,
                        type_params_sqparen,
                    ];

                    vec![Ok(con_def), Ok(con_hash), Ok(type_def)]
                }
            }
        } else {
            let dyn_special_case = if self.module.name
                == eco_format!("{PRELUDE_MODULE_NAME}/option")
                && name == "Option"
            {
                Some(DynamicSpecialCases::Option)
            } else {
                None
            };

            let compatible_fields = get_compatible_record_fields(constructors)
                .into_iter()
                .map(|(_, label, _)| label)
                .collect::<Vec<_>>();

            let compatible_fields_with_types = compatible_fields
                .iter()
                .map(|&label| {
                    let field = constructors[0]
                        .arguments
                        .iter()
                        .find(|arg| arg.label.is_some() && &arg.label.clone().unwrap().1 == label)
                        .unwrap();
                    (label.to_owned(), field.type_.clone())
                })
                .collect::<Vec<_>>();

            let compatible_field_docs =
                compatible_fields_with_types.iter().map(|(label, type_)| {
                    docvec![
                        to_go_common_field_name(label, cons_public, false, false),
                        "() ",
                        type_doc(&self.module, type_, &mut self.tracker, &generic_ids),
                    ]
                });

            let cast_docs = constructors.iter().flat_map(|con| {
                let clean_name = EcoString::from(con.name.to_upper_camel_case());
                let go_name = to_go_constructor_name(&con.name, cons_public);
                let is_doc = docvec![
                    if cons_public { "Is" } else { "is" },
                    clean_name.clone(),
                    "() ",
                    to_go_package_name(PRELUDE_MODULE_NAME),
                    ".Bool_t"
                ];
                let as_doc = docvec![
                    if cons_public { "As" } else { "as" },
                    clean_name,
                    "() ",
                    go_name,
                    type_params_sqparen.clone()
                ];
                vec![is_doc, as_doc]
            });

            let def_doc = match &external_go {
                Some((go_pkg, go_name)) => {
                    if go_pkg == "" && go_name == &type_name {
                        return vec![];
                    }
                    docvec![
                        "= ",
                        if go_pkg != "" {
                            docvec![to_go_package_name(go_pkg), "."]
                        } else {
                            nil()
                        },
                        go_name,
                        type_params_sqparen.clone()
                    ]
                }
                None => {
                    let dyn_doc = match dyn_special_case {
                        Some(DynamicSpecialCases::Option) => vec!["Option_dyn".to_doc()],
                        None => vec![],
                    };

                    docvec![
                        "interface",
                        wrap_curly_semi(
                            std::iter::once(docvec!["i", &type_name, type_params_paren.clone()])
                                .chain(compatible_field_docs)
                                .chain(cast_docs)
                                .chain(dyn_doc)
                                .chain(std::iter::once(docvec![
                                    to_go_package_name(PRELUDE_MODULE_NAME),
                                    ".Type[",
                                    &type_name,
                                    type_params_sqparen.clone(),
                                    "]"
                                ]))
                        )
                    ]
                }
            };

            let type_def = Ok(docvec![
                "type ",
                &type_name,
                type_params_full_doc.clone(),
                " ",
                def_doc,
            ]);

            let dyn_interface_def = match dyn_special_case {
                Some(DynamicSpecialCases::Option) => vec![Ok(docvec![
                    "type Option_dyn interface {",
                    docvec![
                        line(),
                        "ToDynamic() (",
                        to_go_package_name(PRELUDE_MODULE_NAME),
                        ".Dynamic_t, bool)"
                    ]
                    .nest(INDENT),
                    line(),
                    "}"
                ])],
                None => vec![],
            };

            let cons_defs = if external_go.is_some() {
                vec![]
            } else {
                constructors
                    .iter()
                    .enumerate()
                    .flat_map(|(con_idx, con)| {
                        let con_name = to_go_constructor_name(&con.name, cons_public);
                        let con_def_doc = Ok(docvec![
                            "type ",
                            &con_name,
                            type_params_full_doc.clone(),
                            " struct ",
                            if con.arguments.len() == 0 {
                                "{}".to_doc()
                            } else {
                                wrap_curly_semi(con.arguments.iter().enumerate().map(|(i, arg)| {
                                    docvec![
                                        arg.label
                                            .as_ref()
                                            .map(|(_, s)| to_go_field_name(s, cons_public))
                                            .unwrap_or(to_go_positional_field_name(
                                                i.try_into().unwrap(),
                                                cons_public
                                            )),
                                        " ",
                                        type_doc(
                                            &self.module,
                                            &arg.type_,
                                            &mut self.tracker,
                                            &HashSet::new()
                                        ),
                                    ]
                                }))
                            }
                        ]);
                        let con_interface_impl_doc = Ok(docvec![
                            "func (",
                            &con_name,
                            type_params_sqparen.clone(),
                            ") i",
                            &type_name,
                            type_params_paren.clone(),
                            " {}",
                        ]);
                        let common_field_docs = compatible_fields_with_types
                            .iter()
                            .map(|(label, type_)| {
                                Ok(docvec![
                                    "func (c ",
                                    &con_name,
                                    type_params_sqparen.clone(),
                                    ") ",
                                    to_go_common_field_name(label, cons_public, false, false),
                                    "() ",
                                    type_doc(&self.module, type_, &mut self.tracker, &generic_ids),
                                    " { return c.",
                                    to_go_field_name(label, cons_public),
                                    " }",
                                ])
                            })
                            .collect::<Vec<_>>();
                        let cast_docs = constructors.iter().map(|con2| {
                            let clean_name = EcoString::from(con2.name.to_upper_camel_case());
                            let go_name = to_go_constructor_name(&con2.name, cons_public);
                            let is_doc = docvec![
                                "func (",
                                &con_name,
                                type_params_sqparen.clone(),
                                ") ",
                                if cons_public { "Is" } else { "is" },
                                clean_name.clone(),
                                "() ",
                                to_go_package_name(PRELUDE_MODULE_NAME),
                                ".Bool_t { return ",
                                if con.name == con2.name {
                                    "true"
                                } else {
                                    "false"
                                },
                                " }",
                            ];
                            let as_doc = docvec![
                                "func (c ",
                                &con_name,
                                type_params_sqparen.clone(),
                                ") ",
                                if cons_public { "As" } else { "as" },
                                clean_name.clone(),
                                "() ",
                                go_name,
                                type_params_sqparen.clone(),
                                " { ",
                                if con.name == con2.name {
                                    "return c".to_doc()
                                } else {
                                    docvec!["panic(\"expected ", clean_name, " value\")"]
                                },
                                " }",
                            ];
                            Ok(docvec![is_doc, line(), as_doc])
                        });
                        let dyn_interface_impl_doc = match dyn_special_case {
                            Some(DynamicSpecialCases::Option) => {
                                if con.name == "Some" {
                                    vec![Ok(docvec![
                                        "func (c Some_c[T]) ToDynamic() (",
                                        to_go_package_name(PRELUDE_MODULE_NAME),
                                        ".Dynamic_t, bool) { return ",
                                        to_go_package_name(PRELUDE_MODULE_NAME),
                                        ".Dynamic_t{c.P_0}, true }"
                                    ])]
                                } else {
                                    vec![Ok(docvec![
                                        "func (None_c[T]) ToDynamic() (",
                                        to_go_package_name(PRELUDE_MODULE_NAME),
                                        ".Dynamic_t, bool) { return ",
                                        to_go_package_name(PRELUDE_MODULE_NAME),
                                        ".Dynamic_t{}, false }"
                                    ])]
                                }
                            }
                            None => vec![],
                        };

                        let con_hash_doc = Ok(docvec![
                            "func (c ",
                            &con_name,
                            type_params_sqparen.clone(),
                            ") Hash() uint32 { return ",
                            to_go_package_name(PRELUDE_MODULE_NAME),
                            ".HashConstructor(",
                            con_idx,
                            if con.arguments.is_empty() {
                                nil()
                            } else {
                                docvec![
                                    ", ",
                                    comma_separated_list(con.arguments.iter().enumerate().map(
                                        |(arg_idx, arg)| {
                                            Ok(docvec![
                                                "c.",
                                                arg.label
                                                    .as_ref()
                                                    .map(|(_, s)| to_go_common_field_name(
                                                        s,
                                                        cons_public,
                                                        true,
                                                        false
                                                    ))
                                                    .unwrap_or(to_go_positional_field_name(
                                                        arg_idx.try_into().unwrap(),
                                                        cons_public
                                                    )),
                                                ".Hash()",
                                            ])
                                        }
                                    ))
                                    .unwrap(),
                                ]
                            },
                            ") }",
                            line(),
                            "func (c ",
                            &con_name,
                            type_params_sqparen.clone(),
                            ") Equal(o ",
                            &type_name,
                            type_params_sqparen.clone(),
                            ") bool {",
                            docvec![
                                line(),
                                "if o, ok := o.(",
                                &con_name,
                                type_params_sqparen.clone(),
                                "); ok {",
                                docvec![
                                    line(),
                                    "_ = o",
                                    con.arguments
                                        .iter()
                                        .enumerate()
                                        .map(|(arg_idx, arg)| {
                                            let lbl = arg
                                                .label
                                                .as_ref()
                                                .map(|(_, s)| to_go_field_name(s, cons_public))
                                                .unwrap_or(to_go_positional_field_name(
                                                    arg_idx.try_into().unwrap(),
                                                    cons_public,
                                                ));
                                            docvec![
                                                line(),
                                                "if !c.",
                                                &lbl,
                                                ".Equal(o.",
                                                &lbl,
                                                ") { return false }"
                                            ]
                                        })
                                        .collect::<Vec<_>>(),
                                    line(),
                                    "return true"
                                ]
                                .nest(INDENT),
                                line(),
                                "}",
                                line(),
                                "return false"
                            ]
                            .nest(INDENT),
                            line(),
                            "}",
                        ]);

                        std::iter::once(con_def_doc)
                            .chain(std::iter::once(con_interface_impl_doc))
                            .chain(common_field_docs)
                            .chain(cast_docs)
                            .chain(dyn_interface_impl_doc)
                            .chain(std::iter::once(con_hash_doc))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            };

            std::iter::once(type_def)
                .chain(dyn_interface_def)
                .chain(cons_defs)
                .collect()
        }
    }

    // fn record_definition(
    //     &self,
    //     constructor: &'a TypedRecordConstructor,
    //     publicity: Publicity,
    //     opaque: bool,
    // ) -> Document<'a> {
    //     fn parameter((i, arg): (usize, &TypedRecordConstructorArg)) -> Document<'_> {
    //         arg.label
    //             .as_ref()
    //             .map(|(_, s)| maybe_escape_identifier_doc(s))
    //             .unwrap_or_else(|| eco_format!("x{i}").to_doc())
    //     }

    //     let head = if publicity.is_private() || opaque {
    //         "class "
    //     } else {
    //         "export class "
    //     };
    //     let head = docvec![head, &constructor.name, " extends $CustomType {"];

    //     if constructor.arguments.is_empty() {
    //         return head.append("}");
    //     };

    //     let parameters = join(
    //         constructor.arguments.iter().enumerate().map(parameter),
    //         break_(",", ", "),
    //     );

    //     let constructor_body = join(
    //         constructor.arguments.iter().enumerate().map(|(i, arg)| {
    //             let var = parameter((i, arg));
    //             match &arg.label {
    //                 None => docvec!["this[", i, "] = ", var, ";"],
    //                 Some((_, name)) => {
    //                     docvec!["this.", maybe_escape_property_doc(name), " = ", var, ";"]
    //                 }
    //             }
    //         }),
    //         line(),
    //     );

    //     let class_body = docvec![
    //         line(),
    //         "constructor(",
    //         parameters,
    //         ") {",
    //         docvec![line(), "super();", line(), constructor_body].nest(INDENT),
    //         line(),
    //         "}",
    //     ]
    //     .nest(INDENT);

    //     docvec![head, class_body, line(), "}"]
    // }

    fn collect_definitions(&mut self) -> Vec<Output<'a>> {
        self.module
            .definitions
            .iter()
            .flat_map(|statement| match statement {
                Definition::CustomType(CustomType {
                    name,
                    publicity,
                    constructors,
                    opaque,
                    parameters,
                    typed_parameters,
                    external_go,
                    ..
                }) => self.custom_type_definition(
                    name,
                    *publicity,
                    constructors,
                    *opaque,
                    &parameters
                        .iter()
                        .map(|(_, param)| param.to_owned())
                        .collect::<Vec<_>>(),
                    &typed_parameters,
                    external_go
                        .as_ref()
                        .map(|(go_pkg, go_name, _)| (go_pkg.clone(), go_name.clone())),
                ),

                Definition::Function(Function { .. })
                | Definition::TypeAlias(TypeAlias { .. })
                | Definition::Import(Import { .. })
                | Definition::ModuleConstant(ModuleConstant { .. }) => vec![],
            })
            .collect()
    }

    fn collect_imports(&mut self) -> Imports<'a> {
        let mut imports = Imports::new();

        for statement in &self.module.definitions {
            match statement {
                Definition::Import(Import {
                    module,
                    as_name,
                    unqualified_values: unqualified,
                    package,
                    ..
                }) => {
                    self.register_import(&mut imports, package, module, as_name, unqualified);
                    // match as_name {
                    //     Some((AssignName::Variable(name), _)) => {
                    //         let _ = self.aliased_module_names.insert(module, name);
                    //     }
                    //     Some((AssignName::Discard(_), _)) | None => (),
                    // }
                }

                Definition::Function(Function {
                    name: Some((_, name)),
                    publicity,
                    external_go: Some((module, function, _location)),
                    ..
                }) => {
                    if !module.is_empty() {
                        self.register_external_function(
                            &mut imports,
                            *publicity,
                            name,
                            module,
                            function,
                        );
                    }
                }

                Definition::Function(Function {
                    arguments,
                    return_type,
                    body,
                    ..
                }) => {
                    for a in arguments {
                        self.collect_imports_for_type(&a.type_, &mut imports);
                    }
                    self.collect_imports_for_type(return_type, &mut imports);

                    for stmt in body {
                        self.collect_imports_for_stmt(stmt, &mut imports);
                    }
                }

                Definition::TypeAlias(TypeAlias { type_, .. }) => {
                    self.collect_imports_for_type(type_, &mut imports)
                }

                Definition::CustomType(CustomType {
                    constructors,
                    typed_parameters,
                    external_go,
                    ..
                }) => {
                    if let Some((go_pkg, _, _)) = external_go {
                        if go_pkg != "" {
                            self.register_external_type(&mut imports, go_pkg);
                        }
                    }

                    for t in typed_parameters {
                        self.collect_imports_for_type(t, &mut imports);
                    }

                    for constructor in constructors {
                        for arg in constructor.arguments.as_slice() {
                            self.collect_imports_for_type(&arg.type_, &mut imports);
                        }
                    }
                }

                Definition::ModuleConstant(ModuleConstant { type_, value, .. }) => {
                    self.collect_imports_for_type(type_, &mut imports);
                    self.collect_imports_for_const(value, &mut imports);
                }
            }
        }

        imports
    }

    /// Recurses through a type and any types it references, registering all of their imports.
    ///
    fn collect_imports_for_type<'b>(&mut self, type_: &'b Type, imports: &mut Imports<'a>) {
        match &type_ {
            Type::Named {
                package,
                module,
                args,
                ..
            } => {
                let is_prelude = module == "gleam" && package.is_empty();
                let is_current_module = *module == self.module.name;

                if !is_prelude && !is_current_module {
                    self.register_import_simple(imports, package, module);
                }

                for arg in args {
                    self.collect_imports_for_type(arg, imports);
                }
            }
            Type::Fn { args, retrn } => {
                for arg in args {
                    self.collect_imports_for_type(arg, imports);
                }
                self.collect_imports_for_type(retrn, imports);
            }
            Type::Tuple { elems } => {
                for elem in elems {
                    self.collect_imports_for_type(elem, imports);
                }
            }
            Type::Var { type_ } => {
                if let TypeVar::Link { type_ } = type_
                    .as_ref()
                    .try_borrow()
                    .expect("borrow type after inference")
                    .deref()
                {
                    self.collect_imports_for_type(type_, imports);
                }
            }
        }
    }

    fn collect_imports_for_stmt(&mut self, stmt: &TypedStatement, imports: &mut Imports<'a>) {
        match stmt {
            Statement::Expression(expr) => self.collect_imports_for_expr(expr, imports),
            Statement::Assignment(assignment) => {
                self.collect_imports_for_expr(&assignment.value, imports)
            }
            Statement::Use(use_) => self.collect_imports_for_expr(&use_.call, imports),
        }
    }

    fn collect_imports_for_const(&mut self, constant: &TypedConstant, imports: &mut Imports<'a>) {
        match constant {
            Constant::Int { .. } => {}
            Constant::Float { .. } => {}
            Constant::String { .. } => {}
            Constant::Tuple { elements, .. } => {
                for element in elements {
                    self.collect_imports_for_const(element, imports);
                }
            }
            Constant::List {
                elements, type_, ..
            } => {
                self.collect_imports_for_type(type_, imports);
                for element in elements {
                    self.collect_imports_for_const(element, imports);
                }
            }
            Constant::Record { args, type_, .. } => {
                self.collect_imports_for_type(type_, imports);
                for arg in args {
                    self.collect_imports_for_const(&arg.value, imports);
                }
            }
            Constant::BitArray { segments, .. } => {
                for segment in segments {
                    self.collect_imports_for_const(&segment.value, imports);
                }
            }
            Constant::Var {
                constructor, type_, ..
            } => {
                if let Some(constructor) = constructor {
                    self.collect_imports_for_type(&constructor.type_, imports);
                }
                self.collect_imports_for_type(type_, imports);
            }
            Constant::StringConcatenation { left, right, .. } => {
                self.collect_imports_for_const(left, imports);
                self.collect_imports_for_const(right, imports);
            }
            Constant::Invalid { .. } => {}
        }
    }

    fn collect_imports_for_expr(&mut self, expr: &TypedExpr, imports: &mut Imports<'a>) {
        match expr {
            TypedExpr::Int { .. } => {}
            TypedExpr::Float { .. } => {}
            TypedExpr::String { .. } => {}
            TypedExpr::Block { statements, .. } => {
                for stmt in statements {
                    self.collect_imports_for_stmt(stmt, imports);
                }
            }
            TypedExpr::Pipeline {
                assignments,
                finally,
                ..
            } => {
                for assignment in assignments {
                    self.collect_imports_for_expr(&assignment.value, imports);
                }
                self.collect_imports_for_expr(finally, imports);
            }
            TypedExpr::Var { constructor, .. } => {
                self.collect_imports_for_type(&constructor.type_, imports)
            }
            TypedExpr::Fn {
                type_, args, body, ..
            } => {
                self.collect_imports_for_type(type_, imports);
                for arg in args {
                    self.collect_imports_for_type(&arg.type_, imports);
                }
                for stmt in body {
                    self.collect_imports_for_stmt(stmt, imports);
                }
            }
            TypedExpr::List {
                type_,
                elements,
                tail,
                ..
            } => {
                self.collect_imports_for_type(type_, imports);
                for element in elements {
                    self.collect_imports_for_expr(element, imports);
                }
                if let Some(tail) = tail {
                    self.collect_imports_for_expr(tail, imports);
                }
            }
            TypedExpr::Call { fun, args, .. } => {
                self.collect_imports_for_expr(fun, imports);
                for arg in args {
                    self.collect_imports_for_expr(&arg.value, imports);
                }
            }
            TypedExpr::BinOp { left, right, .. } => {
                self.collect_imports_for_expr(left, imports);
                self.collect_imports_for_expr(right, imports);
            }
            TypedExpr::Case {
                subjects, clauses, ..
            } => {
                for subject in subjects {
                    self.collect_imports_for_expr(subject, imports);
                }
                for clause in clauses {
                    self.collect_imports_for_expr(&clause.then, imports);
                }
            }
            TypedExpr::RecordAccess { record, .. } => {
                self.collect_imports_for_expr(record, imports)
            }
            TypedExpr::ModuleSelect { type_, .. } => {
                self.collect_imports_for_type(type_, imports);
            }
            TypedExpr::Tuple { type_, elems, .. } => {
                self.collect_imports_for_type(type_, imports);
                for elem in elems {
                    self.collect_imports_for_expr(elem, imports);
                }
            }
            TypedExpr::TupleIndex { tuple, .. } => {
                self.collect_imports_for_expr(tuple, imports);
            }
            TypedExpr::Todo { .. } => {}
            TypedExpr::Panic { .. } => {}
            TypedExpr::BitArray { segments, .. } => {
                for segment in segments {
                    self.collect_imports_for_expr(&segment.value, imports);
                }
            }
            TypedExpr::RecordUpdate {
                type_,
                record,
                constructor,
                args,
                ..
            } => {
                self.collect_imports_for_type(type_, imports);
                self.collect_imports_for_expr(&record.value, imports);
                self.collect_imports_for_expr(constructor, imports);
                for arg in args {
                    self.collect_imports_for_expr(&arg.value, imports);
                }
            }
            TypedExpr::NegateBool { value, .. } => self.collect_imports_for_expr(value, imports),
            TypedExpr::NegateInt { value, .. } => self.collect_imports_for_expr(value, imports),
            TypedExpr::Invalid { .. } => {}
        }
    }

    fn import_path(&self, package: &'a str, module: &'a str) -> EcoString {
        if package.is_empty() {
            eco_format!("{}/{}", self.go_module_path, module)
        } else {
            eco_format!("{}/{}/{}", self.go_module_path, package, module)
        }
    }

    /// Registers an import of an external module so that it can be added to
    /// the top of the generated Document. The module names are prefixed with a
    /// "$" symbol to prevent any clashes with other Gleam names that may be
    /// used in this module.
    ///
    fn register_import_simple<'b>(
        &mut self,
        imports: &mut Imports<'a>,
        package: &'b str,
        module: &'b str,
    ) {
        let path = self.import_path(package, module);
        imports.register_module(path, [module.into()], []);
    }

    fn register_import(
        &mut self,
        imports: &mut Imports<'a>,
        package: &'a str,
        module: &'a str,
        as_name: &'a Option<(AssignName, SrcSpan)>,
        unqualified: &'a [UnqualifiedImport],
    ) {
        let get_name = |module: &'a str| {
            module
                .split('/')
                .last()
                .expect("Go generator could not identify imported module name.")
        };

        let (discarded, module_name) = match as_name {
            None => (false, get_name(module)),
            Some((AssignName::Discard(_), _)) => (true, get_name(module)),
            Some((AssignName::Variable(name), _)) => (false, name.as_str()),
        };

        let module_name = eco_format!("{module_name}");
        let path = self.import_path(package, module);
        let unqualified_imports = unqualified.iter().map(|i| {
            let alias = i.as_name.as_ref().map(|n| {
                self.register_in_scope(true, n);
                maybe_escape_identifier_doc(n)
            });
            let name = maybe_escape_identifier_doc(&i.name);
            Member { name, alias }
        });

        let aliases = if discarded {
            vec![module.replace("/", "ʹ").into()]
        } else {
            vec![module_name]
        };
        if !discarded || !unqualified.is_empty() {
            imports.register_module(path, aliases, unqualified_imports);
        }
    }

    fn register_external_type(&mut self, imports: &mut Imports<'a>, module: &'a str) {
        let (alias, path) = match module.split_once(' ') {
            Some((alias, path)) => (alias, path),
            None => (module.split('/').last().unwrap(), module),
        };
        imports.register_module(path.into(), [alias.into()], []);
    }

    fn register_external_function(
        &mut self,
        imports: &mut Imports<'a>,
        _publicity: Publicity,
        _name: &'a str,
        module: &'a str,
        _fun: &'a str,
    ) {
        let (alias, path) = match module.split_once(' ') {
            Some((alias, path)) => (alias, path),
            None => (module.split('/').last().unwrap(), module),
        };
        imports.register_module(path.into(), [alias.into()], []);
    }

    fn module_constant(
        &mut self,
        publicity: Publicity,
        name: &'a str,
        value: &'a TypedConstant,
    ) -> Output<'a> {
        let go_name = to_go_name(name, publicity.is_public());

        let document = expression::constant_expression(
            &self.dep_modules,
            &self.module,
            &mut self.tracker,
            value,
            &self.module_scope,
            &HashSet::new(),
        )?;

        Ok(docvec![
            "var ",
            go_name,
            " ",
            type_doc(
                &self.module,
                &value.type_(),
                &mut self.tracker,
                &HashSet::new()
            ),
            " = ",
            document,
        ])
    }

    fn register_in_scope(&mut self, public: bool, name: &str) {
        let _ = self.module_scope.insert(name.into(), (public, 0));
    }

    fn module_function(&mut self, function: &'a TypedFunction) -> Option<Output<'a>> {
        let (_, name) = function
            .name
            .as_ref()
            .expect("A module's function must be named");

        let go_name = to_go_name(name, function.publicity.is_public());

        let generic_ids = collect_generic_usages(
            HashSet::new(),
            std::iter::once(&function.return_type)
                .chain(function.arguments.iter().map(|a| &a.type_)),
        );
        let generic_names: Vec<Document<'_>> = generic_ids
            .iter()
            .sorted()
            .map(|id| id_to_type_var(*id))
            .collect();

        let argument_names = function
            .arguments
            .iter()
            .map(|arg| arg.names.get_variable_name())
            .collect::<Vec<_>>();

        let mut generator = expression::Generator::new(
            &self.dep_modules,
            &self.module,
            self.line_numbers,
            name.clone(),
            argument_names.clone(),
            &mut self.tracker,
            self.module_scope.clone(),
            &generic_ids,
        );

        let body = match &function.external_go {
            Some((external_module, external_name, _)) => {
                if external_module == "" && external_name == &go_name {
                    return None;
                }
                let go_pkg = match external_module.split_once(' ') {
                    Some((alias, _)) => alias,
                    None => external_module.split('/').last().unwrap(),
                };
                let argument_names = argument_names
                    .iter()
                    .filter_map(|n| n.map(|n| to_go_name(n, false).to_doc()));

                docvec![
                    "return ",
                    if go_pkg != "" {
                        docvec![to_go_package_name(go_pkg), "."]
                    } else {
                        nil()
                    },
                    external_name,
                    if generic_names.is_empty() {
                        nil()
                    } else {
                        wrap_generic_args(generic_names.clone())
                    },
                    wrap_args(argument_names),
                ]
            }
            None => match generator.function_body(&function.body, function.arguments.as_slice()) {
                // No error, let's continue!
                Ok(body) => body,

                // There is an error coming from some expression that is not supported on Go
                // and the target support is not enforced. In this case we do not error, instead
                // returning nothing which will cause no function to be generated.
                Err(error) if error.is_unsupported() && !self.target_support.is_enforced() => {
                    return None
                }

                // Some other error case which will be returned to the user.
                Err(error) => return Some(Err(error)),
            },
        };

        let tail_recursion_used = generator.tail_recursion_used;
        let args = fun_args(
            &self.module,
            function.arguments.as_slice(),
            tail_recursion_used,
            &mut self.tracker,
            &generic_ids,
        );
        let document = docvec![
            "func ",
            go_name,
            if generic_names.is_empty() {
                nil()
            } else {
                wrap_generic_params(generic_names)
            },
            args,
            " ",
            type_doc(
                &self.module,
                &function.return_type,
                &mut self.tracker,
                &generic_ids
            ),
            " {",
            docvec![line(), body].nest(INDENT).group(),
            line(),
            "}",
        ];
        Some(Ok(document))
    }

    fn register_module_definitions_in_scope(&mut self) {
        for statement in self.module.definitions.iter() {
            match statement {
                Definition::ModuleConstant(ModuleConstant {
                    name, publicity, ..
                }) => self.register_in_scope(publicity.is_public(), name),

                Definition::Function(Function {
                    name, publicity, ..
                }) => self.register_in_scope(
                    publicity.is_public(),
                    name.as_ref()
                        .map(|(_, name)| name)
                        .expect("Function in a definition must be named"),
                ),

                Definition::Import(Import {
                    unqualified_values: unqualified,
                    ..
                }) => unqualified
                    .iter()
                    .for_each(|unq_import| self.register_in_scope(true, unq_import.used_name())),

                Definition::TypeAlias(TypeAlias { .. })
                | Definition::CustomType(CustomType { .. }) => (),
            }
        }
    }
}

/// Prints a "named" programmer-defined Gleam type into the Go equivalent.
///
fn print_type_app(
    self_module: &TypedModule,
    public: bool,
    name: &str,
    args: &[Arc<Type>],
    module: &str,
    tracker: &mut UsageTracker,
    generic_ids_in_scope: &HashSet<u64>,
) -> Document<'static> {
    let name = to_go_type_name(name, public);
    let name = match module == self_module.name {
        true => name.to_doc(),
        false => {
            // If type comes from a separate module, use that module's name
            // as a Go namespace prefix
            docvec![to_go_package_name(module), ".", name]
        }
    };
    if args.is_empty() {
        return name;
    }

    // If the App type takes arguments, pass them in as Go generics
    docvec![
        name,
        wrap_generic_args(args.iter().map(|a| type_doc(
            self_module,
            a,
            tracker,
            generic_ids_in_scope
        )))
    ]
}

fn fun_args<'a>(
    self_module: &TypedModule,
    args: &'a [TypedArg],
    tail_recursion_used: bool,
    tracker: &'_ mut UsageTracker,
    generic_ids_in_scope: &'_ HashSet<u64>,
) -> Document<'a> {
    wrap_args(args.iter().map(|a| {
        let arg = match a.get_variable_name() {
            None => "_".to_doc(),
            Some(name) if tail_recursion_used => {
                eco_format!("loop_{}", to_go_name(name, false)).to_doc()
            }
            Some(name) => to_go_name(name, false).to_doc(),
        };
        let type_ = type_doc(self_module, &a.type_, tracker, generic_ids_in_scope);
        docvec![arg, " ", type_]
    }))
}

fn type_doc(
    self_module: &TypedModule,
    type_: &Type,
    tracker: &mut UsageTracker,
    generic_ids_in_scope: &HashSet<u64>,
) -> Document<'static> {
    match type_ {
        Type::Named {
            module, name, args, ..
        } if is_prelude_module(module) => {
            print_prelude_type(self_module, name, args, tracker, generic_ids_in_scope)
        }
        Type::Named {
            publicity,
            name,
            args,
            module,
            ..
        } => print_type_app(
            self_module,
            publicity.is_public(),
            name,
            args,
            module,
            tracker,
            generic_ids_in_scope,
        ),
        Type::Fn { args, retrn } => {
            let args_docs = args
                .iter()
                .map(|a| Ok(type_doc(self_module, a, tracker, generic_ids_in_scope)))
                .collect::<Vec<_>>();
            let ret_doc = vec![Ok(type_doc(
                self_module,
                retrn,
                tracker,
                generic_ids_in_scope,
            ))];
            docvec![
                to_go_package_name(PRELUDE_MODULE_NAME),
                ".Func",
                args.len(),
                "_t[",
                comma_separated_list(args_docs.into_iter().chain(ret_doc)).unwrap(),
                "]",
            ]
        }
        Type::Var { type_ } => match type_.borrow().deref() {
            TypeVar::Unbound { id } => {
                if generic_ids_in_scope.contains(id) {
                    id_to_type_var(*id)
                } else {
                    docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".Type"]
                }
            }
            TypeVar::Link { type_ } => type_doc(self_module, type_, tracker, generic_ids_in_scope),
            TypeVar::Generic { id } => id_to_type_var(*id),
        },
        Type::Tuple { elems } => tuple_type(self_module, elems, tracker, generic_ids_in_scope),
    }
}

fn tuple_type(
    self_module: &TypedModule,
    elem_types: &Vec<Arc<Type>>,
    tracker: &mut UsageTracker,
    generic_ids_in_scope: &HashSet<u64>,
) -> Document<'static> {
    let num_elems = elem_types.len();
    tracker.tuple_used = true;
    docvec![
        to_go_package_name(PRELUDE_MODULE_NAME),
        eco_format!(".Tuple{num_elems}_t"),
        if num_elems > 0 {
            docvec![wrap_sq_comma(elem_types.iter().map(|t| {
                type_doc(self_module, &t, tracker, generic_ids_in_scope)
            })),]
        } else {
            nil()
        }
    ]
}

/// Prints a type coming from the Gleam prelude module. These are often the
/// low level types the rest of the type system are built up from. If there
/// is no built-in Go equivalent, the type is prefixed with "_."
/// and the Gleam prelude namespace will be imported during the code emission.
///
fn print_prelude_type(
    self_module: &TypedModule,
    name: &str,
    args: &[Arc<Type>],
    tracker: &mut UsageTracker,
    generic_ids_in_scope: &HashSet<u64>,
) -> Document<'static> {
    match name {
        "Nil" => {
            tracker.nil_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".Nil_t",]
        }
        "Int" => {
            tracker.nil_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".Int_t",]
        }
        "Float" => {
            tracker.nil_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".Float_t",]
        }
        "UtfCodepoint" => {
            tracker.nil_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".UtfCodepoint_t",]
        }
        "String" => {
            tracker.nil_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".String_t",]
        }
        "Bool" => {
            tracker.nil_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".Bool_t",]
        }
        "BitArray" => {
            tracker.bit_array_used = true;
            docvec![to_go_package_name(PRELUDE_MODULE_NAME), ".BitArray_t"]
        }
        "List" => {
            tracker.list_used = true;
            docvec![
                to_go_package_name(PRELUDE_MODULE_NAME),
                ".List_t",
                wrap_generic_args(args.iter().map(|x| type_doc(
                    self_module,
                    x,
                    tracker,
                    generic_ids_in_scope
                )))
            ]
        }
        "Result" => {
            tracker.result_used = true;
            docvec![
                to_go_package_name(PRELUDE_MODULE_NAME),
                ".Result_t",
                wrap_generic_args(args.iter().map(|x| type_doc(
                    self_module,
                    x,
                    tracker,
                    generic_ids_in_scope
                )))
            ]
        }
        // // Getting here should mean we either forgot a built-in type or there is a
        // // compiler error
        name => panic!("{name} is not a built-in type."),
    }
}

fn collect_generic_usages<'a>(
    mut ids: HashSet<u64>,
    types: impl IntoIterator<Item = &'a Arc<Type>>,
) -> HashSet<u64> {
    for type_ in types {
        generic_ids(type_, &mut ids);
    }
    ids
}

fn generic_ids(type_: &Type, ids: &mut HashSet<u64>) {
    match type_ {
        Type::Var { type_ } => match type_.borrow().deref() {
            TypeVar::Unbound { id, .. } | TypeVar::Generic { id, .. } => {
                let _ = ids.insert(*id);
            }
            TypeVar::Link { type_ } => generic_ids(type_, ids),
        },
        Type::Named { args, .. } => {
            for arg in args {
                generic_ids(arg, ids)
            }
        }
        Type::Fn { args, retrn } => {
            for arg in args {
                generic_ids(arg, ids)
            }
            generic_ids(retrn, ids);
        }
        Type::Tuple { elems } => {
            for elem in elems {
                generic_ids(elem, ids)
            }
        }
    }
}

/// Converts a usize into base 26 A-Z.
fn id_to_type_var(id: u64) -> Document<'static> {
    if id < 26 {
        return std::iter::once(
            std::char::from_u32((id % 26 + 65) as u32).expect("id_to_type_var 0"),
        )
        .collect::<EcoString>()
        .to_doc();
    }
    let mut name = vec![];
    let mut last_char = id;
    while last_char >= 26 {
        name.push(std::char::from_u32((last_char % 26 + 65) as u32).expect("id_to_type_var 1"));
        last_char /= 26;
    }
    name.push(std::char::from_u32((last_char % 26 + 64) as u32).expect("id_to_type_var 2"));
    name.reverse();
    name.into_iter().collect::<EcoString>().to_doc()
}

fn wrap_generic_params<'a, I>(args: I) -> Document<'a>
where
    I: IntoIterator<Item = Document<'a>>,
{
    break_("", "")
        .append(join(
            args.into_iter().map(|arg| {
                docvec![
                    arg.clone(),
                    " ",
                    to_go_package_name(PRELUDE_MODULE_NAME),
                    ".Type[",
                    arg,
                    "]"
                ]
            }),
            break_(",", ", "),
        ))
        .nest(INDENT)
        .append(break_(",", ""))
        .surround("[", "]")
        .group()
}

fn wrap_generic_args<'a, I>(args: I) -> Document<'a>
where
    I: IntoIterator<Item = Document<'a>>,
{
    break_("", "")
        .append(join(args, break_(",", ", ")))
        .nest(INDENT)
        .append(break_(",", ""))
        .surround("[", "]")
        .group()
}

pub fn module<'a>(
    dep_modules: &im::HashMap<EcoString, ModuleInterface>,
    module: &'a TypedModule,
    line_numbers: &'a LineNumbers,
    path: &Utf8Path,
    src: &EcoString,
    target_support: TargetSupport,
    go_module_path: &'a str,
) -> Result<String, crate::Error> {
    let document = Generator::new(
        dep_modules,
        line_numbers,
        module,
        target_support,
        go_module_path,
    )
    .compile()
    .map_err(|error| crate::Error::Go {
        path: path.to_path_buf(),
        src: src.clone(),
        error,
    })?;
    Ok(document.to_pretty_string(80))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unsupported { feature: String, location: SrcSpan },
}

impl Error {
    /// Returns `true` if the error is [`Unsupported`].
    ///
    /// [`Unsupported`]: Error::Unsupported
    #[must_use]
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported { .. })
    }
}

fn wrap_args<'a, I>(args: I) -> Document<'a>
where
    I: IntoIterator<Item = Document<'a>>,
{
    break_("", "")
        .append(join(args, break_(",", ", ")))
        .nest(INDENT)
        .append(break_(",", ""))
        .surround("(", ")")
        .group()
}

fn wrap_curly_semi<'a, I>(args: I) -> Document<'a>
where
    I: IntoIterator<Item = Document<'a>>,
{
    break_("", "")
        .append(join(args, break_("", "; ")))
        .nest(INDENT)
        .append(break_("", ""))
        .surround("{", "}")
        .group()
}

fn wrap_sq_comma<'a, I>(args: I) -> Document<'a>
where
    I: IntoIterator<Item = Document<'a>>,
{
    break_("", "")
        .append(join(args, break_(",", ", ")))
        .nest(INDENT)
        .append(break_(",", ""))
        .surround("[", "]")
        .group()
}

fn wrap_object<'a>(
    items: impl IntoIterator<Item = (Document<'a>, Option<Document<'a>>)>,
) -> Document<'a> {
    let mut empty = true;
    let fields = items.into_iter().map(|(key, value)| {
        empty = false;
        match value {
            Some(value) => docvec!["\"", key, "\": ", value],
            None => key.to_doc(),
        }
    });
    let fields = join(fields, break_(",", ", "));

    if empty {
        "{}".to_doc()
    } else {
        docvec![
            docvec!["{", break_("", ""), fields]
                .nest(INDENT)
                .append(break_("", " "))
                .group(),
            "}"
        ]
    }
}

fn is_usable_go_identifier(word: &str) -> bool {
    !matches!(
        word,
        // --- https://go.dev/ref/spec#Keywords ---
        "break"        | "default"      | "func"         | "interface"    | "select"
        | "case"         | "defer"        | "go"           | "map"          | "struct"
        | "chan"         | "else"         | "goto"         | "package"      | "switch"
        | "const"        | "fallthrough"  | "if"           | "range"        | "type"
        | "continue"     | "for"          | "import"       | "return"       | "var"

        // --- https://go.dev/ref/spec#Predeclared_identifiers ---
        // Types:
        | "any" | "bool" | "byte" | "comparable"
        | "complex64" | "complex128" | "error" | "float32" | "float64"
        | "int" | "int8" | "int16" | "int32" | "int64" | "rune" | "string"
        | "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "uintptr"

        // Constants:
        | "true" | "false" | "iota"

        // Zero value:
        | "nil"

        // Functions:
        | "append" | "cap" | "clear" | "close" | "complex" | "copy" | "delete" | "imag" | "len"
        | "make" | "max" | "min" | "new" | "panic" | "print" | "println" | "real" | "recover"

        // --- Other ---
        // A top-level `init` must have a specific type and is evaluated automatically at startup
        | "init"

        // Used packages
        | "strings"

        // gleam-go internals
        | "Hash" | "Equal"
    )
}

fn maybe_escape_identifier_string(word: &str) -> EcoString {
    if is_usable_go_identifier(word) {
        EcoString::from(word)
    } else {
        escape_identifier(word)
    }
}

fn escape_string_identifier(word: EcoString) -> EcoString {
    eco_format!("{word}ʹ")
}

fn escape_identifier(word: &str) -> EcoString {
    eco_format!("{word}ʹ")
}

fn maybe_escape_string_identifier_doc<'a>(word: EcoString) -> Document<'a> {
    if is_usable_go_identifier(&word) {
        word.to_doc()
    } else {
        escape_string_identifier(word).to_doc()
    }
}

fn maybe_escape_identifier_doc(word: &str) -> Document<'_> {
    if is_usable_go_identifier(word) {
        word.to_doc()
    } else {
        escape_identifier(word).to_doc()
    }
}

fn to_go_package_name(name: &str) -> EcoString {
    eco_format!(
        "{}_P",
        maybe_escape_identifier_string(name.split('/').last().unwrap())
    )
}

fn to_go_name(name: &str, public: bool) -> EcoString {
    let base = if public {
        name.to_upper_camel_case()
    } else {
        name.to_lower_camel_case()
    };
    if name.starts_with("_") {
        eco_format!("_{base}")
    } else {
        maybe_escape_identifier_string(&base)
    }
}

fn to_go_type_name(name: &str, public: bool) -> EcoString {
    let mut base = if public {
        name.to_owned()
    } else {
        name.to_lower_camel_case()
    };
    base.push_str("_t");
    if name.starts_with("_") {
        base.insert(0, '_');
    }
    maybe_escape_identifier_string(&base)
}

fn to_go_constructor_name(name: &str, public: bool) -> EcoString {
    let mut base = if public {
        name.to_upper_camel_case()
    } else {
        name.to_lower_camel_case()
    };
    base.push_str("_c");
    if name.starts_with("_") {
        base.insert(0, '_');
    }
    maybe_escape_identifier_string(&base)
}

fn to_go_field_name(name: &str, public: bool) -> EcoString {
    to_go_name(name, public)
}

fn to_go_common_field_name(
    name: &str,
    public: bool,
    single_constructor: bool,
    access: bool,
) -> EcoString {
    if single_constructor {
        to_go_field_name(name, public)
    } else {
        eco_format!(
            "{}_f{}",
            to_go_name(name, public),
            if access { "()" } else { "" }
        )
    }
}

fn to_go_positional_field_name(i: u64, public: bool) -> EcoString {
    if public {
        eco_format!("P_{i}")
    } else {
        eco_format!("p_{i}")
    }
}

#[derive(Debug, Default)]
pub(crate) struct UsageTracker {
    pub go_strings_used: bool,
    pub go_fmt_used: bool,
    pub tuple_used: bool,
    pub nil_used: bool,
    pub int_used: bool,
    pub float_used: bool,
    pub string_used: bool,
    pub bool_used: bool,
    pub result_used: bool,
    pub ok_used: bool,
    pub list_used: bool,
    pub prepend_used: bool,
    pub error_used: bool,
    pub int_remainder_used: bool,
    pub make_error_used: bool,
    pub custom_type_used: bool,
    pub int_division_used: bool,
    pub float_division_used: bool,
    pub object_equality_used: bool,
    pub bit_array_used: bool,
    pub utf_codepoint_used: bool,
    pub bit_array_literal_used: bool,
    pub sized_integer_segment_used: bool,
    pub string_bit_array_segment_used: bool,
    pub codepoint_bit_array_segment_used: bool,
    pub float_bit_array_segment_used: bool,
}

fn bool(bool: bool) -> Document<'static> {
    match bool {
        true => "true".to_doc(),
        false => "false".to_doc(),
    }
}

/// Int segments <= 64 bits wide in bit arrays are within JavaScript's safe range and are evaluated
/// at compile time when all inputs are known. This is done for both bit array expressions and
/// pattern matching.
///
/// Int segments of any size could be evaluated at compile time, but currently aren't due to the
/// potential for causing large generated Go for inputs such as `<<0:8192>>`.
///
pub(crate) const SAFE_INT_SEGMENT_MAX_SIZE: usize = 48;

/// Evaluates the value of an Int segment in a bit array into its corresponding bytes. This avoids
/// needing to do the evaluation at runtime when all inputs are known at compile-time.
///
pub(crate) fn bit_array_segment_int_value_to_bytes(
    mut value: BigInt,
    size: BigInt,
    endianness: endianness::Endianness,
) -> Result<Vec<u8>, Error> {
    // Clamp negative sizes to zero
    let size = size.max(BigInt::ZERO);

    // Convert size to u32. This is safe because this function isn't called with a size greater
    // than `SAFE_INT_SEGMENT_MAX_SIZE`.
    let size = size
        .to_u32()
        .expect("bit array segment size to be a valid u32");

    // Convert negative number to two's complement representation
    if value < BigInt::ZERO {
        let value_modulus = BigInt::from(2).pow(size);
        value = &value_modulus + (value % &value_modulus);
    }

    // Convert value to the desired number of bytes
    let mut bytes = vec![0u8; size as usize / 8];
    for byte in bytes.iter_mut() {
        *byte = (&value % BigInt::from(256))
            .to_u8()
            .expect("modulo result to be a valid u32");
        value /= BigInt::from(256);
    }

    if endianness.is_big() {
        bytes.reverse();
    }

    Ok(bytes)
}
