use num_bigint::BigInt;
use regex::Regex;
use vec1::Vec1;

use super::{
    pattern::{Assignment, CompiledPattern},
    *,
};
use crate::{
    ast::*,
    go::endianness::Endianness,
    line_numbers::LineNumbers,
    pretty::*,
    type_::{
        collapse_links, ModuleValueConstructor, Type, TypedCallArg, ValueConstructor,
        ValueConstructorVariant,
    },
};
use std::sync::{Arc, LazyLock};

#[derive(Debug, Clone, Copy)]
pub enum Position {
    Tail,
    NotTail,
}

impl Position {
    /// Returns `true` if the position is [`Tail`].
    ///
    /// [`Tail`]: Position::Tail
    #[must_use]
    pub fn is_tail(&self) -> bool {
        matches!(self, Self::Tail)
    }
}

#[derive(Debug)]
pub(crate) struct Generator<'module> {
    pub dep_modules: &'module im::HashMap<EcoString, ModuleInterface>,
    pub module: &'module TypedModule,
    line_numbers: &'module LineNumbers,
    function_name: Option<EcoString>,
    function_arguments: Vec<Option<&'module EcoString>>,
    current_scope_vars: im::HashMap<EcoString, (bool, usize)>,
    pub generic_type_ids_in_scope: &'module HashSet<u64>,
    pub function_position: Position,
    pub scope_position: Position,
    // We register whether these features are used within an expression so that
    // the module generator can output a suitable function if it is needed.
    pub tracker: &'module mut UsageTracker,
    // We track whether tail call recursion is used so that we can render a loop
    // at the top level of the function to use in place of pushing new stack
    // frames.
    pub tail_recursion_used: bool,
}

impl<'module> Generator<'module> {
    #[allow(clippy::too_many_arguments)] // TODO: FIXME
    pub fn new(
        dep_modules: &'module im::HashMap<EcoString, ModuleInterface>,
        module: &'module TypedModule,
        line_numbers: &'module LineNumbers,
        function_name: EcoString,
        function_arguments: Vec<Option<&'module EcoString>>,
        tracker: &'module mut UsageTracker,
        mut current_scope_vars: im::HashMap<EcoString, (bool, usize)>,
        generic_type_ids_in_scope: &'module HashSet<u64>,
    ) -> Self {
        let mut function_name = Some(function_name);
        for &name in function_arguments.iter().flatten() {
            // Initialise the function arguments
            let _ = current_scope_vars.insert(name.clone(), (false, 0));

            // If any of the function arguments shadow the current function then
            // recursion is no longer possible.
            if function_name.as_ref() == Some(name) {
                function_name = None;
            }
        }
        Self {
            dep_modules,
            module,
            tracker,
            line_numbers,
            function_name,
            function_arguments,
            tail_recursion_used: false,
            current_scope_vars,
            function_position: Position::Tail,
            scope_position: Position::Tail,
            generic_type_ids_in_scope,
        }
    }

    pub fn local_var<'a>(&mut self, name: &'a EcoString) -> Document<'a> {
        match self.current_scope_vars.get(name) {
            None => {
                let _ = self.current_scope_vars.insert(name.clone(), (false, 0));
                maybe_escape_identifier_doc(name)
            }
            Some((public, 0)) => maybe_escape_string_identifier_doc(to_go_name(name, *public)),
            Some((_, n)) if name == "$" => eco_format!("ʹ{n}").to_doc(),
            Some((_, n)) => eco_format!("{name}ʹ{n}").to_doc(),
        }
    }

    pub fn next_local_var<'a>(&mut self, name: &'a EcoString) -> Document<'a> {
        let next = self
            .current_scope_vars
            .get(name)
            .map_or((false, 0), |(public, i)| (*public, i + 1));
        let _ = self.current_scope_vars.insert(name.clone(), next);
        self.local_var(name)
    }

    pub fn function_body<'a>(
        &mut self,
        body: &'a [TypedStatement],
        args: &'a [TypedArg],
    ) -> Output<'a> {
        let body = self.statements(body)?;
        if self.tail_recursion_used {
            self.tail_call_loop(body, args)
        } else {
            Ok(body)
        }
    }

    fn tail_call_loop<'a>(&mut self, body: Document<'a>, args: &'a [TypedArg]) -> Output<'a> {
        let loop_assignments = concat(
            args.iter()
                .flat_map(|arg| {
                    arg.get_variable_name()
                        .map(|name| (name, arg.type_.clone()))
                })
                .map(|(name, type_)| {
                    let var = to_go_name(name, false);
                    docvec![
                        "var ",
                        &var,
                        " ",
                        type_doc(
                            &self.module,
                            &type_,
                            &mut self.tracker,
                            &self.generic_type_ids_in_scope
                        ),
                        " = loop_",
                        &var,
                        line()
                    ]
                }),
        );
        Ok(docvec![
            "for {",
            docvec![line(), loop_assignments, body].nest(INDENT),
            line(),
            "}"
        ])
    }

    fn statement<'a>(&mut self, statement: &'a TypedStatement) -> Output<'a> {
        match statement {
            Statement::Expression(expression) => self.expression(expression, true),
            Statement::Assignment(assignment) => self.assignment(assignment),
            Statement::Use(_use) => self.expression(&_use.call, true),
        }
    }

    pub fn expression<'a>(&mut self, expression: &'a TypedExpr, unused: bool) -> Output<'a> {
        let document = match expression {
            TypedExpr::String { value, .. } => self.force_use(Ok(string(value)), unused),

            TypedExpr::Int { value, .. } => self.force_use(Ok(int(value)), unused),
            TypedExpr::Float { value, .. } => self.force_use(Ok(float(value)), unused),

            TypedExpr::List {
                elements,
                tail,
                type_,
                ..
            } => {
                let elem_type = list_element_type(&type_);
                let elem_type_doc = type_doc(
                    &self.module,
                    &elem_type,
                    &mut self.tracker,
                    self.generic_type_ids_in_scope,
                );
                let res = self.not_in_tail_position(|gen| match tail {
                    Some(tail) => {
                        gen.tracker.prepend_used = true;
                        let tail = gen.wrap_expression(tail)?;
                        prepend(
                            elements.iter().map(|e| gen.wrap_expression(e)),
                            elem_type_doc.clone(),
                            tail,
                        )
                    }
                    None => {
                        gen.tracker.list_used = true;
                        list(
                            elements.iter().map(|e| gen.wrap_expression(e)),
                            elem_type_doc.clone(),
                        )
                    }
                });
                self.force_use(res, unused)
            }
            TypedExpr::Tuple { elems, .. } => {
                let res = self.tuple(elems);
                self.force_use(res, unused)
            }
            TypedExpr::TupleIndex { tuple, index, .. } => {
                let res = self.tuple_index(tuple, *index);
                self.force_use(res, unused)
            }
            TypedExpr::Case {
                subjects, clauses, ..
            } => self.case(subjects, clauses, unused),

            TypedExpr::Call { fun, args, .. } => {
                let res = self.call(fun, args);
                self.force_use(res, unused)
            }
            TypedExpr::Fn {
                args, body, type_, ..
            } => {
                let res = self.fn_(args, body, type_.clone());
                self.force_use(res, unused)
            }

            TypedExpr::RecordAccess { record, label, .. } => {
                let res = self.record_access(record, label);
                self.force_use(res, unused)
            }
            TypedExpr::RecordUpdate {
                record,
                constructor,
                args,
                ..
            } => self.record_update(record, constructor, args, unused),

            TypedExpr::Var {
                name, constructor, ..
            } => {
                let res = self.variable(name, constructor);
                self.force_use(res, unused)
            }

            TypedExpr::Pipeline {
                assignments,
                finally,
                ..
            } => self.pipeline(assignments.as_slice(), finally, unused),

            TypedExpr::Block { statements, .. } => {
                let res = self.block(statements);
                self.force_use(res, unused)
            }

            TypedExpr::BinOp {
                name, left, right, ..
            } => {
                let res = self.bin_op(name, left, right);
                self.force_use(res, unused)
            }

            TypedExpr::Todo {
                message, location, ..
            } => self.todo(message.as_ref().map(|m| &**m), location),

            TypedExpr::Panic {
                location, message, ..
            } => self.panic(location, message.as_ref().map(|m| &**m)),

            TypedExpr::BitArray { segments, .. } => {
                let res = self.bit_array(segments);
                self.force_use(res, unused)
            }

            TypedExpr::ModuleSelect {
                module_alias,
                label,
                constructor,
                type_,
                ..
            } => {
                let res = Ok(self.module_select(module_alias, label, constructor, type_.clone()));
                self.force_use(res, unused)
            }

            TypedExpr::NegateBool { value, .. } => {
                let res = self.negate_with("!", value);
                self.force_use(res, unused)
            }

            TypedExpr::NegateInt { value, .. } => {
                let res = self.negate_with("- ", value);
                self.force_use(res, unused)
            }

            TypedExpr::Invalid { .. } => {
                panic!("invalid expressions should not reach code generation")
            }
        }?;
        Ok(if expression.handles_own_return_go() {
            document
        } else {
            self.wrap_return(document)
        })
    }

    fn negate_with<'a>(&mut self, with: &'static str, value: &'a TypedExpr) -> Output<'a> {
        self.not_in_tail_position(|gen| Ok(docvec![with, gen.wrap_expression(value)?]))
    }

    fn bit_array<'a>(&mut self, segments: &'a [TypedExprBitArraySegment]) -> Output<'a> {
        self.tracker.bit_array_literal_used = true;

        use BitArrayOption as Opt;

        // Collect all the values used in segments.
        let segments_array = comma_separated_list(segments.iter().map(|segment| {
            let value = self.not_in_tail_position(|gen| gen.wrap_expression(&segment.value))?;

            if segment.type_ == crate::type_::int() || segment.type_ == crate::type_::float() {
                let details = self.sized_bit_array_segment_details(segment)?;

                if segment.type_ == crate::type_::int() {
                    match (details.size_value, segment.value.as_ref()) {
                        (Some(size_value), TypedExpr::Int { int_value, .. })
                            if size_value <= SAFE_INT_SEGMENT_MAX_SIZE.into() =>
                        {
                            let bytes = bit_array_segment_int_value_to_bytes(
                                int_value.clone(),
                                size_value,
                                details.endianness,
                            )?;

                            Ok(u8_slice(&bytes))
                        }

                        (Some(size_value), _) if size_value == 8.into() => {
                            Ok(docvec!["[]byte{byte(", value, ")}"])
                        }

                        (Some(size_value), _) if size_value <= 0.into() => Ok(nil()),

                        _ => {
                            self.tracker.sized_integer_segment_used = true;
                            Ok(docvec![
                                to_go_package_name(PRELUDE_MODULE_NAME),
                                ".SizedInt(",
                                value,
                                ", ",
                                details.size,
                                ", ",
                                bool(details.endianness.is_big()),
                                ")"
                            ])
                        }
                    }
                } else {
                    self.tracker.float_bit_array_segment_used = true;
                    Ok(docvec![
                        to_go_package_name(PRELUDE_MODULE_NAME),
                        ".SizedFloat(",
                        value,
                        ", ",
                        details.size,
                        ", ",
                        bool(details.endianness.is_big()),
                        ")"
                    ])
                }
            } else {
                match segment.options.as_slice() {
                    // UTF8 strings
                    [Opt::Utf8 { .. }] => {
                        self.tracker.string_bit_array_segment_used = true;
                        Ok(docvec![
                            to_go_package_name(PRELUDE_MODULE_NAME),
                            ".StringBits(",
                            value,
                            ")"
                        ])
                    }

                    // UTF8 codepoints
                    [Opt::Utf8Codepoint { .. }] => {
                        self.tracker.codepoint_bit_array_segment_used = true;
                        Ok(docvec![
                            to_go_package_name(PRELUDE_MODULE_NAME),
                            ".CodepointBits(",
                            value,
                            ")"
                        ])
                    }

                    // Bit arrays
                    [Opt::Bytes { .. } | Opt::Bits { .. }] => Ok(docvec![value, ".Buffer()"]),

                    // Anything else
                    _ => Err(Error::Unsupported {
                        feature: "This bit array segment option".into(),
                        location: segment.location,
                    }),
                }
            }
        }))?;
        Ok(docvec![
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".ToBitArray(",
            segments_array,
            ")"
        ])
    }

    fn sized_bit_array_segment_details<'a>(
        &mut self,
        segment: &'a TypedExprBitArraySegment,
    ) -> Result<SizedBitArraySegmentDetails<'a>, Error> {
        use BitArrayOption as Opt;

        if segment
            .options
            .iter()
            .any(|x| matches!(x, Opt::Native { .. }))
        {
            return Err(Error::Unsupported {
                feature: "This bit array segment option".into(),
                location: segment.location,
            });
        }

        let endianness = if segment
            .options
            .iter()
            .any(|x| matches!(x, Opt::Little { .. }))
        {
            Endianness::Little
        } else {
            Endianness::Big
        };

        let size = segment
            .options
            .iter()
            .find(|x| matches!(x, Opt::Size { .. }));

        let (size_value, size) = match size {
            Some(Opt::Size { value: size, .. }) => {
                let size_value = match *size.clone() {
                    TypedExpr::Int { int_value, .. } => Some(int_value),
                    _ => None,
                };

                if let Some(size_value) = size_value.as_ref() {
                    if *size_value > BigInt::ZERO && size_value % 8 != BigInt::ZERO {
                        return Err(Error::Unsupported {
                            feature: "Non byte aligned array".into(),
                            location: segment.location,
                        });
                    }
                }

                (
                    size_value,
                    self.not_in_tail_position(|gen| gen.wrap_expression(size))?,
                )
            }
            _ => {
                let size_value = if segment.type_ == crate::type_::int() {
                    8usize
                } else {
                    64usize
                };

                (Some(BigInt::from(size_value)), docvec![size_value])
            }
        };

        Ok(SizedBitArraySegmentDetails {
            size,
            size_value,
            endianness,
        })
    }

    pub fn wrap_return<'a>(&self, document: Document<'a>) -> Document<'a> {
        if self.scope_position.is_tail() {
            docvec!["return ", document]
        } else {
            document
        }
    }

    fn force_use<'a>(&self, doc: Output<'a>, unused: bool) -> Output<'a> {
        if !self.scope_position.is_tail() && unused {
            Ok(docvec!["_ = ", doc?])
        } else {
            doc
        }
    }

    pub fn not_in_tail_position<'a, CompileFn>(&mut self, compile: CompileFn) -> Output<'a>
    where
        CompileFn: Fn(&mut Self) -> Output<'a>,
    {
        let function_position = self.function_position;
        let scope_position = self.scope_position;

        self.function_position = Position::NotTail;
        self.scope_position = Position::NotTail;
        let result = compile(self);

        self.function_position = function_position;
        self.scope_position = scope_position;
        result
    }

    /// Wrap an expression in an immediately involked function expression if
    /// required due to being a Go statement
    pub fn wrap_expression<'a>(&mut self, expression: &'a TypedExpr) -> Output<'a> {
        match expression {
            TypedExpr::Panic { .. }
            | TypedExpr::Todo { .. }
            | TypedExpr::Case { .. }
            | TypedExpr::Pipeline { .. }
            | TypedExpr::RecordUpdate { .. } => self.immediately_invoked_function_expression(
                expression,
                expression.type_(),
                |gen, expr| gen.expression(expr, false),
            ),
            _ => self.expression(expression, false),
        }
    }

    /// Wrap an expression in an immediately involked function expression if
    /// required due to being a Go statement, or in parens if required due to
    /// being an operator or a function literal.
    pub fn child_expression<'a>(&mut self, expression: &'a TypedExpr) -> Output<'a> {
        match expression {
            TypedExpr::BinOp { name, .. } if name.is_operator_to_wrap_go() => {}
            TypedExpr::Fn { .. } => {}

            _ => return self.wrap_expression(expression),
        }

        let document = self.expression(expression, false)?;
        Ok(if self.scope_position.is_tail() {
            // Here the document is a return statement: `return <expr>;`
            document
        } else {
            docvec!["(", document, ")"]
        })
    }

    /// Wrap an expression in an immediately involked function expression
    fn immediately_invoked_function_expression<'a, T, ToDoc>(
        &mut self,
        statements: &'a T,
        type_: Arc<Type>,
        to_doc: ToDoc,
    ) -> Output<'a>
    where
        ToDoc: FnOnce(&mut Self, &'a T) -> Output<'a>,
    {
        // Save initial state
        let scope_position = self.scope_position;

        // Set state for in this iife
        self.scope_position = Position::Tail;
        let current_scope_vars = self.current_scope_vars.clone();

        // Generate the expression
        let result = to_doc(self, statements);

        // Reset
        self.current_scope_vars = current_scope_vars;
        self.scope_position = scope_position;

        // Wrap in iife document
        let doc = immediately_invoked_function_expression_document(
            &self.module,
            result?,
            type_,
            &mut self.tracker,
            self.generic_type_ids_in_scope,
        );
        Ok(self.wrap_return(doc))
    }

    fn variable<'a>(
        &mut self,
        name: &'a EcoString,
        constructor: &'a ValueConstructor,
    ) -> Output<'a> {
        match &constructor.variant {
            ValueConstructorVariant::LocalConstant { literal } => constant_expression(
                &self.dep_modules,
                &self.module,
                self.tracker,
                literal,
                &self.current_scope_vars,
                self.generic_type_ids_in_scope,
            ),
            ValueConstructorVariant::Record { arity, module, .. } => {
                let type_ = constructor.type_.clone();
                let tracker = &mut self.tracker;
                Ok(record_constructor(
                    &self.module,
                    type_,
                    if module != &self.module.name {
                        Some(module.clone())
                    } else {
                        None
                    },
                    name.clone(),
                    *arity,
                    tracker,
                    self.generic_type_ids_in_scope,
                ))
            }
            ValueConstructorVariant::ModuleFn { .. } => self.module_fn(
                constructor.publicity.is_public(),
                name,
                constructor.type_.clone(),
            ),
            ValueConstructorVariant::ModuleConstant { .. } => self.module_const(
                constructor.publicity.is_public(),
                name,
                constructor.type_.clone(),
            ),
            ValueConstructorVariant::LocalVariable { .. } => Ok(self.local_var(name)),
        }
    }

    fn module_fn<'a>(&mut self, public: bool, name: &'a EcoString, type_: Arc<Type>) -> Output<'a> {
        let (module, name, generic_type) = self
            .module
            .definitions
            .iter()
            .find_map(|def| match def {
                Definition::Function(function) => match &function.name {
                    Some(def_name) => {
                        if &def_name.1 == name {
                            Some((
                                nil(),
                                name,
                                Arc::new(Type::Fn {
                                    args: function
                                        .arguments
                                        .iter()
                                        .map(|arg| arg.type_.clone())
                                        .collect(),
                                    retrn: function.return_type.clone(),
                                }),
                            ))
                        } else {
                            None
                        }
                    }
                    None => None,
                },
                Definition::TypeAlias(_type_alias) => None,
                Definition::CustomType(_custom_type) => None,
                Definition::Import(import) => import.unqualified_values.iter().find_map(|value| {
                    let imported_name = value.as_name.as_ref().unwrap_or(&value.name);
                    if imported_name != name {
                        None
                    } else {
                        let origin_module = self
                            .dep_modules
                            .get(&import.module)
                            .expect("dependent module exists");

                        Some((
                            docvec![
                                &to_go_package_name(
                                    &import
                                        .used_name()
                                        .unwrap_or(import.module.clone())
                                        .replace("/", "ʹ")
                                ),
                                "."
                            ],
                            &value.name,
                            origin_module
                                .get_public_value(&value.name)
                                .expect("value exists")
                                .type_
                                .clone(),
                        ))
                    }
                }),
                Definition::ModuleConstant(_module_constant) => None,
            })
            .expect("function definition");

        let mut id_map = im::HashMap::new();
        solve_type_apps(generic_type, type_, &mut id_map);
        let type_args = id_map
            .iter()
            .sorted_by_key(|(k, _)| *k)
            .map(|(_, v)| {
                type_doc(
                    self.module,
                    &v,
                    &mut self.tracker,
                    self.generic_type_ids_in_scope,
                )
            })
            .collect::<Vec<_>>();

        if type_args.is_empty() {
            Ok(docvec![module, to_go_name(name, public)])
        } else {
            Ok(docvec![
                module,
                to_go_name(name, public),
                wrap_generic_args(type_args)
            ])
        }
    }

    fn module_const<'a>(
        &mut self,
        public: bool,
        name: &'a EcoString,
        type_: Arc<Type>,
    ) -> Output<'a> {
        let (module, name, generic_type) = self
            .module
            .definitions
            .iter()
            .find_map(|def| match def {
                Definition::Function(_) => None,
                Definition::TypeAlias(_type_alias) => None,
                Definition::CustomType(_custom_type) => None,
                Definition::Import(import) => import.unqualified_values.iter().find_map(|value| {
                    let imported_name = value.as_name.as_ref().unwrap_or(&value.name);
                    if imported_name != name {
                        None
                    } else {
                        let origin_module = self
                            .dep_modules
                            .get(&import.module)
                            .expect("dependent module exists");

                        Some((
                            docvec![
                                &to_go_package_name(
                                    &import.used_name().unwrap_or(import.module.clone())
                                ),
                                "."
                            ],
                            &value.name,
                            origin_module
                                .get_public_value(&value.name)
                                .expect("value exists")
                                .type_
                                .clone(),
                        ))
                    }
                }),
                Definition::ModuleConstant(module_constant) => {
                    let def_name = &module_constant.name;
                    if def_name == name {
                        Some((nil(), name, module_constant.type_.clone()))
                    } else {
                        None
                    }
                }
            })
            .expect("constant definition");

        let mut id_map = im::HashMap::new();
        solve_type_apps(generic_type, type_, &mut id_map);
        let type_args = id_map
            .iter()
            .sorted_by_key(|(k, _)| *k)
            .map(|(_, v)| {
                type_doc(
                    self.module,
                    &v,
                    &mut self.tracker,
                    self.generic_type_ids_in_scope,
                )
            })
            .collect::<Vec<_>>();

        if type_args.is_empty() {
            Ok(docvec![module, to_go_name(name, public)])
        } else {
            Ok(docvec![
                module,
                to_go_name(name, public),
                wrap_generic_args(type_args)
            ])
        }
    }

    fn pipeline<'a>(
        &mut self,
        assignments: &'a [TypedPipelineAssignment],
        finally: &'a TypedExpr,
        unused: bool,
    ) -> Output<'a> {
        let count = assignments.len();
        let mut documents = Vec::with_capacity((count + 1) * 2);
        for assignment in assignments.iter() {
            documents.push(self.not_in_tail_position(|gen| {
                gen.simple_variable_assignment(&assignment.name, &assignment.value)
            })?);
            documents.push(line());
        }
        documents.push(self.expression(finally, unused)?);
        Ok(documents.to_doc().force_break())
    }

    fn expression_flattening_blocks<'a>(
        &mut self,
        expression: &'a TypedExpr,
        unused: bool,
    ) -> Output<'a> {
        match expression {
            TypedExpr::Block { statements, .. } => self.statements(statements),
            _ => self.expression(expression, unused),
        }
    }

    fn block<'a>(&mut self, statements: &'a Vec1<TypedStatement>) -> Output<'a> {
        if self.scope_position.is_tail() {
            // If the block is in tail position there's no need to wrap it in an
            // immediately invoked function expression; we can just return its
            // last expression.
            self.statements(statements)
        } else if statements.len() == 1 {
            match statements.first() {
                Statement::Expression(expression) => self.child_expression(expression),

                Statement::Assignment(assignment) => {
                    self.child_expression(assignment.value.as_ref())
                }

                Statement::Use(use_) => self.child_expression(&use_.call),
            }
        } else {
            let type_ = statements.last().type_();
            self.immediately_invoked_function_expression(statements, type_, |gen, statements| {
                gen.statements(statements)
            })
        }
    }

    fn statements<'a>(&mut self, statements: &'a [TypedStatement]) -> Output<'a> {
        let count = statements.len();
        let mut documents = Vec::with_capacity(count * 3);
        for (i, statement) in statements.iter().enumerate() {
            if i + 1 < count {
                documents.push(self.not_in_tail_position(|gen| gen.statement(statement))?);
                documents.push(line());
            } else {
                documents.push(self.statement(statement)?);
            }
        }
        if count == 1 {
            Ok(documents.to_doc())
        } else {
            Ok(documents.to_doc().force_break())
        }
    }

    fn simple_variable_assignment<'a>(
        &mut self,
        name: &'a EcoString,
        value: &'a TypedExpr,
    ) -> Output<'a> {
        // Subject must be rendered before the variable for variable numbering
        let subject = self.not_in_tail_position(|gen| gen.wrap_expression(value))?;
        let go_name = self.next_local_var(name);
        let assignment = docvec![
            "var ",
            go_name.clone(),
            " ",
            type_doc(
                &self.module,
                &value.type_(),
                &mut self.tracker,
                self.generic_type_ids_in_scope
            ),
            " = ",
            subject,
            line(),
            "_ = ", // TODO: track variable usage and choose `_ = ...` or `var x T = ...` accordingly
            go_name.clone(),
        ];
        let assignment = if self.scope_position.is_tail() {
            docvec![assignment, line(), "return ", go_name]
        } else {
            assignment
        };

        Ok(assignment.force_break())
    }

    fn assignment<'a>(&mut self, assignment: &'a TypedAssignment) -> Output<'a> {
        let TypedAssignment {
            pattern,
            kind,
            value,
            annotation: _,
            location: _,
        } = assignment;

        // If it is a simple assignment to a variable we can generate a normal
        // Go assignment
        if let TypedPattern::Variable { name, .. } = pattern {
            return self.simple_variable_assignment(name, value);
        }

        // Otherwise we need to compile the patterns
        let (subject, subject_assignment) = pattern::assign_subject(self, value);
        // Value needs to be rendered before traversing pattern to have correctly incremented variables.
        let value = self.not_in_tail_position(|gen| gen.wrap_expression(value))?;
        let mut pattern_generator = pattern::Generator::new(self);
        pattern_generator.traverse_pattern(&subject, pattern)?;
        let compiled = pattern_generator.take_compiled();

        // If we are in tail position we can return value being assigned
        let afterwards = if self.scope_position.is_tail() {
            docvec![
                line(),
                "return ",
                subject_assignment.clone().unwrap_or_else(|| value.clone()),
            ]
        } else {
            nil()
        };

        let compiled =
            self.pattern_into_assignment_doc(compiled, subject, pattern.location(), kind)?;
        // If there is a subject name given create a variable to hold it for
        // use in patterns
        let doc = match subject_assignment {
            Some(name) => docvec![
                "var ",
                name.clone(),
                " ",
                type_doc(
                    &self.module,
                    &assignment.value.type_(),
                    &mut self.tracker,
                    self.generic_type_ids_in_scope
                ),
                " = ",
                value,
                line(),
                "_ = ", // TODO: track variable usage and choose `_ = ...` or `var x T = ...` accordingly
                name,
                line(),
                compiled
            ],
            None => compiled,
        };

        Ok(doc.append(afterwards).force_break())
    }

    fn case<'a>(
        &mut self,
        subject_values: &'a [TypedExpr],
        clauses: &'a [TypedClause],
        unused: bool,
    ) -> Output<'a> {
        let (subjects, subject_assignments): (Vec<_>, Vec<_>) =
            pattern::assign_subjects(self, subject_values)
                .into_iter()
                .unzip();
        let mut gen = pattern::Generator::new(self);

        let mut doc = nil();

        // We wish to be able to know whether this is the first or clause being
        // processed, so record the index number. We use this instead of
        // `Iterator.enumerate` because we are using a nested for loop.
        let mut clause_number = 0;
        let total_patterns: usize = clauses
            .iter()
            .map(|c| c.alternative_patterns.len())
            .sum::<usize>()
            + clauses.len();

        // A case has many clauses `pattern -> consequence`
        for clause in clauses {
            let multipattern = std::iter::once(&clause.pattern);
            let multipatterns = multipattern.chain(&clause.alternative_patterns);

            // A clause can have many patterns `pattern, pattern ->...`
            for multipatterns in multipatterns {
                let scope = gen.expression_generator.current_scope_vars.clone();
                let mut compiled = gen.generate(&subjects, multipatterns, clause.guard.as_ref())?;
                let consequence = gen
                    .expression_generator
                    .expression_flattening_blocks(&clause.then, unused)?;

                // We've seen one more clause
                clause_number += 1;

                // Reset the scope now that this clause has finished, causing the
                // variables to go out of scope.
                gen.expression_generator.current_scope_vars = scope;

                // If the pattern assigns any variables we need to render assignments
                let body = if compiled.has_assignments() {
                    let assignments = gen
                        .expression_generator
                        .pattern_take_assignments_doc(&mut compiled);
                    docvec![assignments, line(), consequence]
                } else {
                    consequence
                };

                let is_final_clause = clause_number == total_patterns;
                let is_first_clause = clause_number == 1;
                let is_only_clause = is_final_clause && is_first_clause;

                doc = if is_only_clause {
                    // If this is the only clause and there are no checks then we can
                    // render just the body as the case does nothing
                    // A block is used as it could declare variables still.
                    doc.append("{")
                        .append(docvec![line(), body].nest(INDENT))
                        .append(line())
                        .append("}")
                } else if is_final_clause {
                    // If this is the final clause and there are no checks then we can
                    // render `else` instead of `else if (...)`
                    doc.append(" else {")
                        .append(docvec![line(), body].nest(INDENT))
                        .append(line())
                        .append("}")
                } else {
                    doc.append(if is_first_clause { "if " } else { " else if " })
                        .append(
                            gen.expression_generator
                                .pattern_take_checks_doc(&mut compiled, true),
                        )
                        .append(" {")
                        .append(docvec![line(), body].nest(INDENT))
                        .append(line())
                        .append("}")
                };
            }
        }

        // If there is a subject name given create a variable to hold it for
        // use in patterns
        let subject_assignments: Vec<_> = subject_assignments
            .into_iter()
            .zip(subject_values)
            .flat_map(|(assignment_name, value)| assignment_name.map(|name| (name, value)))
            .map(|(name, value)| {
                let value_doc = self.not_in_tail_position(|gen| gen.wrap_expression(value))?;
                Ok(docvec![
                    "var ",
                    name.clone(),
                    " ",
                    type_doc(
                        &self.module,
                        &value.type_(),
                        &mut self.tracker,
                        self.generic_type_ids_in_scope
                    ),
                    " = ",
                    value_doc,
                    line(),
                    "_ = ", // TODO: track variable usage and choose `_ = ...` or `var x T = ...` accordingly
                    name,
                    line()
                ])
            })
            .try_collect()?;

        Ok(docvec![subject_assignments, doc].force_break())
    }

    fn assignment_no_match<'a>(
        &mut self,
        location: SrcSpan,
        subject: Document<'a>,
        message: Option<&'a TypedExpr>,
    ) -> Output<'a> {
        let message = match message {
            Some(m) => self.not_in_tail_position(|gen| gen.expression(m, false))?,
            None => string("Pattern match failed, no pattern matched the value."),
        };

        Ok(self.throw_error("let_assert", &message, location, [("value", subject)]))
    }

    fn tuple<'a>(&mut self, elements: &'a [TypedExpr]) -> Output<'a> {
        Ok(docvec![
            tuple_type(
                &self.module,
                &elements.iter().map(|element| element.type_()).collect(),
                &mut self.tracker,
                self.generic_type_ids_in_scope
            ),
            self.not_in_tail_position(|gen| {
                initializer(elements.iter().map(|element| gen.wrap_expression(element)))
            })?,
        ])
    }

    fn call<'a>(&mut self, fun: &'a TypedExpr, arguments: &'a [TypedCallArg]) -> Output<'a> {
        let arguments = arguments
            .iter()
            .map(|element| self.not_in_tail_position(|gen| gen.wrap_expression(&element.value)))
            .try_collect()?;

        self.call_with_doc_args(fun, arguments)
    }

    fn call_with_doc_args<'a>(
        &mut self,
        fun: &'a TypedExpr,
        arguments: Vec<Document<'a>>,
    ) -> Output<'a> {
        match fun {
            // Qualified record construction
            TypedExpr::ModuleSelect {
                constructor: ModuleValueConstructor::Record { name, type_, .. },
                module_alias,
                ..
            } => {
                let (_, _, type_args) = type_
                    .return_type()
                    .expect("function type")
                    .named_type_information()
                    .expect("named type");
                let rec = construct_record(
                    &self.module,
                    &mut self.tracker,
                    &self.generic_type_ids_in_scope,
                    Some(module_alias.clone()),
                    name.to_owned(),
                    &type_args,
                    arguments,
                );
                Ok(self.wrap_return(rec))
            }

            // Record construction
            TypedExpr::Var {
                constructor:
                    ValueConstructor {
                        variant: ValueConstructorVariant::Record { module, .. },
                        type_,
                        ..
                    },
                name,
                ..
            } => {
                if type_.is_result_constructor() {
                    if name == "Ok" {
                        self.tracker.ok_used = true;
                    } else if name == "Error" {
                        self.tracker.error_used = true;
                    }
                } else if type_.is_nil() {
                    self.tracker.nil_used = true;
                }
                let (_, _, type_args) = type_
                    .return_type()
                    .expect("function type")
                    .named_type_information()
                    .expect("named type");

                let rec = construct_record(
                    &self.module,
                    &mut self.tracker,
                    &self.generic_type_ids_in_scope,
                    Some(module.clone()),
                    name.to_owned(),
                    &type_args,
                    arguments,
                );
                Ok(self.wrap_return(rec))
            }

            // Tail call optimisation. If we are calling the current function
            // and we are in tail position we can avoid creating a new stack
            // frame, enabling recursion with constant memory usage.
            TypedExpr::Var { name, .. }
                if self.function_name.as_ref() == Some(name)
                    && self.function_position.is_tail()
                    && (self.current_scope_vars.get(name) == Some(&(false, 0))
                        || self.current_scope_vars.get(name) == Some(&(true, 0))) =>
            {
                let mut docs = Vec::with_capacity(arguments.len() * 4);
                // Record that tail recursion is happening so that we know to
                // render the loop at the top level of the function.
                self.tail_recursion_used = true;

                for (i, (element, argument)) in arguments
                    .into_iter()
                    .zip(&self.function_arguments)
                    .enumerate()
                {
                    if i != 0 {
                        docs.push(line());
                    }
                    // Create an assignment for each variable created by the function arguments
                    if let Some(name) = argument {
                        docs.push("loop_".to_doc());
                        docs.push(to_go_name(name, false).to_doc());
                        docs.push(" = ".to_doc());
                    } else {
                        docs.push("_ = ".to_doc());
                    }
                    // Render the value given to the function. Even if it is not
                    // assigned we still render it because the expression may
                    // have some side effects.
                    docs.push(element);
                }
                Ok(docs.to_doc())
            }

            _ => {
                let fun = self.not_in_tail_position(|gen| {
                    let is_fn_literal = matches!(fun, TypedExpr::Fn { .. });
                    let fun = gen.wrap_expression(fun)?;
                    if is_fn_literal {
                        Ok(docvec!["(", fun, ")"])
                    } else {
                        Ok(fun)
                    }
                })?;
                let arguments = call_arguments(arguments.into_iter().map(Ok))?;
                Ok(self.wrap_return(docvec![fun, arguments]))
            }
        }
    }

    fn fn_<'a>(
        &mut self,
        arguments: &'a [TypedArg],
        body: &'a [TypedStatement],
        type_: Arc<Type>,
    ) -> Output<'a> {
        // New function, this is now the tail position
        let function_position = self.function_position;
        let scope_position = self.scope_position;
        self.function_position = Position::Tail;
        self.scope_position = Position::Tail;

        // And there's a new scope
        let scope = self.current_scope_vars.clone();
        for name in arguments.iter().flat_map(Arg::get_variable_name) {
            let _ = self.current_scope_vars.insert(name.clone(), (false, 0));
        }

        // This is a new function so unset the recorded name so that we don't
        // mistakenly trigger tail call optimisation
        let mut name = None;
        std::mem::swap(&mut self.function_name, &mut name);

        // Generate the function body
        let result = self.statements(body);

        // Reset function name, scope, and tail position tracking
        self.function_position = function_position;
        self.scope_position = scope_position;
        self.current_scope_vars = scope;
        std::mem::swap(&mut self.function_name, &mut name);

        Ok(docvec![
            docvec![
                "func",
                fun_args(
                    &self.module,
                    arguments,
                    false,
                    &mut self.tracker,
                    self.generic_type_ids_in_scope
                ),
                " ",
                type_doc(
                    &self.module,
                    &type_.return_type().expect("must have function type"),
                    &mut self.tracker,
                    self.generic_type_ids_in_scope
                ),
                " {",
                break_("", " "),
                result?
            ]
            .nest(INDENT)
            .append(break_("", " "))
            .group(),
            "}",
        ])
    }

    fn record_access<'a>(&mut self, record: &'a TypedExpr, label: &'a str) -> Output<'a> {
        let (type_module, type_name) = record.type_().named_type_name().expect("named type");

        let public = is_type_public_and_transparent(&self.module, &type_name);
        let single_constructor =
            is_type_single_constructor(&self.dep_modules, &self.module, &type_module, &type_name);

        self.not_in_tail_position(|gen| {
            let record = gen.wrap_expression(record)?;
            Ok(docvec![
                record,
                ".",
                to_go_common_field_name(label, public, single_constructor, true)
            ])
        })
    }

    fn record_update<'a>(
        &mut self,
        record: &'a TypedAssignment,
        constructor: &'a TypedExpr,
        args: &'a [TypedCallArg],
        unused: bool,
    ) -> Output<'a> {
        Ok(docvec![
            self.not_in_tail_position(|gen| gen.assignment(record))?,
            line(),
            if !self.scope_position.is_tail() && unused {
                "_ = ".to_doc()
            } else {
                nil()
            },
            self.call(constructor, args)?,
        ])
    }

    fn tuple_index<'a>(&mut self, tuple: &'a TypedExpr, index: u64) -> Output<'a> {
        self.not_in_tail_position(|gen| {
            let tuple = gen.wrap_expression(tuple)?;
            Ok(docvec![
                tuple,
                ".",
                to_go_positional_field_name(index, true)
            ])
        })
    }

    fn bin_op<'a>(
        &mut self,
        name: &'a BinOp,
        left: &'a TypedExpr,
        right: &'a TypedExpr,
    ) -> Output<'a> {
        match name {
            BinOp::And => self.print_bin_op(left, right, "&&"),
            BinOp::Or => self.print_bin_op(left, right, "||"),
            BinOp::LtInt | BinOp::LtFloat => self.print_bin_op(left, right, "<"),
            BinOp::LtEqInt | BinOp::LtEqFloat => self.print_bin_op(left, right, "<="),
            BinOp::Eq => self.equal(left, right, true),
            BinOp::NotEq => self.equal(left, right, false),
            BinOp::GtInt | BinOp::GtFloat => self.print_bin_op(left, right, ">"),
            BinOp::GtEqInt | BinOp::GtEqFloat => self.print_bin_op(left, right, ">="),
            BinOp::Concatenate | BinOp::AddInt | BinOp::AddFloat => {
                self.print_bin_op(left, right, "+")
            }
            BinOp::SubInt | BinOp::SubFloat => self.print_bin_op(left, right, "-"),
            BinOp::MultInt | BinOp::MultFloat => self.print_bin_op(left, right, "*"),
            BinOp::RemainderInt => self.remainder_int(left, right),
            BinOp::DivInt => self.div_int(left, right),
            BinOp::DivFloat => self.div_float(left, right),
        }
    }

    fn div_int<'a>(&mut self, left: &'a TypedExpr, right: &'a TypedExpr) -> Output<'a> {
        let left = self.not_in_tail_position(|gen| gen.child_expression(left))?;
        let right = self.not_in_tail_position(|gen| gen.child_expression(right))?;
        self.tracker.int_division_used = true;
        Ok(docvec![
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".DivideInt",
            wrap_args([left, right])
        ])
    }

    fn remainder_int<'a>(&mut self, left: &'a TypedExpr, right: &'a TypedExpr) -> Output<'a> {
        let left = self.not_in_tail_position(|gen| gen.child_expression(left))?;
        let right = self.not_in_tail_position(|gen| gen.child_expression(right))?;
        self.tracker.int_remainder_used = true;
        Ok(docvec![
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".RemainderInt",
            wrap_args([left, right])
        ])
    }

    fn div_float<'a>(&mut self, left: &'a TypedExpr, right: &'a TypedExpr) -> Output<'a> {
        let left = self.not_in_tail_position(|gen| gen.child_expression(left))?;
        let right = self.not_in_tail_position(|gen| gen.child_expression(right))?;
        self.tracker.float_division_used = true;
        Ok(docvec![
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".DivideFloat",
            wrap_args([left, right])
        ])
    }

    fn equal<'a>(
        &mut self,
        left: &'a TypedExpr,
        right: &'a TypedExpr,
        should_be_equal: bool,
    ) -> Output<'a> {
        // If it is a simple scalar type then we can use Go's reference identity
        if is_go_scalar(left.type_()) {
            let left_doc = self.not_in_tail_position(|gen| gen.child_expression(left))?;
            let right_doc = self.not_in_tail_position(|gen| gen.child_expression(right))?;
            let negation = if should_be_equal { "" } else { "!" };
            return Ok(docvec![
                to_go_package_name(PRELUDE_MODULE_NAME),
                ".Bool_t(",
                negation,
                left_doc,
                ".Equal(",
                right_doc,
                "))"
            ]);
        }

        // Other types must be compared using structural equality
        let left = self.not_in_tail_position(|gen| gen.wrap_expression(left))?;
        let right = self.not_in_tail_position(|gen| gen.wrap_expression(right))?;
        Ok(self.prelude_equal_call(should_be_equal, left, right))
    }

    pub(super) fn prelude_equal_call<'a>(
        &mut self,
        should_be_equal: bool,
        left: Document<'a>,
        right: Document<'a>,
    ) -> Document<'a> {
        // Record that we need to import the prelude's IsEqual function into the module
        self.tracker.object_equality_used = true;
        // Construct the call
        docvec![
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".Bool_t(",
            if should_be_equal { "" } else { "!" },
            left,
            ".Equal(",
            right,
            "))"
        ]
    }

    fn print_bin_op<'a>(
        &mut self,
        left: &'a TypedExpr,
        right: &'a TypedExpr,
        op: &'a str,
    ) -> Output<'a> {
        let left = self.not_in_tail_position(|gen| gen.child_expression(left))?;
        let right = self.not_in_tail_position(|gen| gen.child_expression(right))?;
        Ok(docvec![left, " ", op, " ", right])
    }

    fn todo<'a>(&mut self, message: Option<&'a TypedExpr>, location: &'a SrcSpan) -> Output<'a> {
        let message = match message {
            Some(m) => self.not_in_tail_position(|gen| gen.expression(m, false))?,
            None => string("`todo` expression evaluated. This code has not yet been implemented."),
        };
        let doc = self.throw_error("todo", &message, *location, vec![]);

        Ok(doc)
    }

    fn panic<'a>(&mut self, location: &'a SrcSpan, message: Option<&'a TypedExpr>) -> Output<'a> {
        let message = match message {
            Some(m) => self.not_in_tail_position(|gen| gen.expression(m, false))?,
            None => string("`panic` expression evaluated."),
        };
        let doc = self.throw_error("panic", &message, *location, vec![]);

        Ok(doc)
    }

    fn throw_error<'a, Fields>(
        &mut self,
        error_name: &'a str,
        message: &Document<'a>,
        location: SrcSpan,
        fields: Fields,
    ) -> Document<'a>
    where
        Fields: IntoIterator<Item = (&'a str, Document<'a>)>,
    {
        self.tracker.make_error_used = true;
        let module = self.module.name.clone().to_doc().surround('"', '"');
        let function = self
            .function_name
            .clone()
            .unwrap_or_default()
            .to_doc()
            .surround("\"", "\"");
        let line = self.line_numbers.line_number(location.start).to_doc();
        let fields = docvec![
            "map[string]any",
            wrap_object(fields.into_iter().map(|(k, v)| (k.to_doc(), Some(v))))
        ];

        docvec![
            "panic(",
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".MakeError",
            wrap_args([
                string(error_name),
                module,
                line,
                function,
                message.clone(),
                fields,
            ]),
            ")"
        ]
    }

    fn module_select<'a>(
        &mut self,
        module: &'a str,
        label: &'a str,
        constructor: &'a ModuleValueConstructor,
        type_: Arc<Type>,
    ) -> Document<'a> {
        match constructor {
            ModuleValueConstructor::Fn { .. } => {
                let (module, generic_type) = self
                    .module
                    .definitions
                    .iter()
                    .find_map(|def| match def {
                        Definition::Function(_function) => None,
                        Definition::TypeAlias(_type_alias) => None,
                        Definition::CustomType(_custom_type) => None,
                        Definition::Import(import) => {
                            if import.used_name() == Some(module.into()) {
                                let origin_module = self
                                    .dep_modules
                                    .get(&import.module)
                                    .expect("dependent module exists");

                                Some((
                                    to_go_package_name(
                                        &import
                                            .used_name()
                                            .unwrap_or(import.module.clone())
                                            .replace("/", "ʹ"),
                                    ),
                                    origin_module
                                        .get_public_value(label)
                                        .expect("value exists")
                                        .type_
                                        .clone(),
                                ))
                            } else {
                                None
                            }
                        }
                        Definition::ModuleConstant(_module_constant) => None,
                    })
                    .expect("function definition");

                let mut id_map = im::HashMap::new();
                solve_type_apps(generic_type, type_, &mut id_map);

                let type_args = id_map
                    .iter()
                    .sorted_by_key(|(k, _)| *k)
                    .map(|(_, v)| {
                        type_doc(
                            self.module,
                            &v,
                            &mut self.tracker,
                            self.generic_type_ids_in_scope,
                        )
                    })
                    .collect::<Vec<_>>();

                let type_args_doc = if type_args.is_empty() {
                    nil()
                } else {
                    wrap_generic_args(type_args)
                };

                docvec![module, ".", to_go_name(label, true), type_args_doc]
            }

            ModuleValueConstructor::Constant { .. } => {
                docvec![to_go_package_name(module), ".", to_go_name(label, true)]
            }

            ModuleValueConstructor::Record {
                name, arity, type_, ..
            } => record_constructor(
                &self.module,
                type_.clone(),
                Some(module.into()),
                name.clone(),
                *arity,
                self.tracker,
                self.generic_type_ids_in_scope,
            ),
        }
    }

    fn pattern_into_assignment_doc<'a>(
        &mut self,
        compiled_pattern: CompiledPattern<'a>,
        subject: Document<'a>,
        location: SrcSpan,
        kind: &'a AssignmentKind<TypedExpr>,
    ) -> Output<'a> {
        let any_assignments = !compiled_pattern.assignments.is_empty();
        let assignments = Self::pattern_assignments_doc(compiled_pattern.assignments);

        // If it's an assert then it is likely that the pattern is inexhaustive. When a value is
        // provided that does not get matched the code needs to throw an exception, which is done
        // by the pattern_checks_or_throw_doc method.
        match kind {
            AssignmentKind::Assert { message, .. } if !compiled_pattern.checks.is_empty() => {
                let checks = self.pattern_checks_or_throw_doc(
                    compiled_pattern.checks,
                    subject,
                    location,
                    message.as_deref(),
                )?;

                if !any_assignments {
                    Ok(checks)
                } else {
                    Ok(docvec![checks, line(), assignments])
                }
            }
            _ => Ok(assignments),
        }
    }

    fn pattern_checks_or_throw_doc<'a>(
        &mut self,
        checks: Vec<pattern::Check<'a>>,
        subject: Document<'a>,
        location: SrcSpan,
        message: Option<&'a TypedExpr>,
    ) -> Output<'a> {
        let checks = self.pattern_checks_doc(checks, false);
        Ok(docvec![
            "if ",
            docvec![break_("", ""), checks].nest(INDENT),
            " {",
            docvec![
                line(),
                self.assignment_no_match(location, subject, message)?
            ]
            .nest(INDENT),
            line(),
            "}",
        ]
        .group())
    }

    fn pattern_assignments_doc(assignments: Vec<Assignment<'_>>) -> Document<'_> {
        let assignments = assignments.into_iter().map(Assignment::into_doc);
        join(assignments, line())
    }

    fn pattern_take_assignments_doc<'a>(
        &self,
        compiled_pattern: &mut CompiledPattern<'a>,
    ) -> Document<'a> {
        let assignments = std::mem::take(&mut compiled_pattern.assignments);
        Self::pattern_assignments_doc(assignments)
    }

    fn pattern_take_checks_doc<'a>(
        &self,
        compiled_pattern: &mut CompiledPattern<'a>,
        match_desired: bool,
    ) -> Document<'a> {
        let checks = std::mem::take(&mut compiled_pattern.checks);
        self.pattern_checks_doc(checks, match_desired)
    }

    fn pattern_checks_doc<'a>(
        &self,
        checks: Vec<pattern::Check<'a>>,
        match_desired: bool,
    ) -> Document<'a> {
        if checks.is_empty() {
            return "true".to_doc();
        };
        let operator = if match_desired {
            break_(" &&", " && ")
        } else {
            break_(" ||", " || ")
        };

        let checks_len = checks.len();
        join(
            checks.into_iter().map(|check| {
                if checks_len > 1 && check.may_require_wrapping() {
                    docvec!["(", check.into_doc(match_desired), ")"]
                } else {
                    check.into_doc(match_desired)
                }
            }),
            operator,
        )
        .group()
    }
}

fn solve_type_apps(
    generic: Arc<Type>,
    instance: Arc<Type>,
    result: &mut im::HashMap<u64, Arc<Type>>,
) {
    let instance = collapse_links(instance);
    match (generic.as_ref(), instance.as_ref()) {
        (Type::Var { type_ }, _) => match &*type_.borrow() {
            TypeVar::Unbound { id } => {
                let _ = result.insert(*id, instance);
            }
            TypeVar::Link { type_ } => solve_type_apps(type_.clone(), instance, result),
            TypeVar::Generic { id } => {
                let _ = result.insert(*id, instance);
            }
        },
        (
            Type::Named {
                package: generic_package,
                module: generic_module,
                name: generic_name,
                args: generic_args,
                ..
            },
            Type::Named {
                package: instance_package,
                module: instance_module,
                name: instance_name,
                args: instance_args,
                ..
            },
        ) if generic_package == instance_package
            && generic_module == instance_module
            && generic_name == instance_name
            && generic_args.len() == instance_args.len() =>
        {
            generic_args
                .iter()
                .zip(instance_args.iter())
                .for_each(|(g, i)| solve_type_apps(g.clone(), i.clone(), result))
        }
        (Type::Named { .. }, _) => panic!(
            "Expected (generic, instance), got ({:?}, {:?})",
            generic, instance
        ),
        (
            Type::Fn {
                args: generic_args,
                retrn: generic_retrn,
            },
            Type::Fn {
                args: instance_args,
                retrn: instance_retrn,
            },
        ) if generic_args.len() == instance_args.len() => {
            generic_args
                .iter()
                .zip(instance_args.iter())
                .for_each(|(g, i)| solve_type_apps(g.clone(), i.clone(), result));
            solve_type_apps(generic_retrn.clone(), instance_retrn.clone(), result)
        }
        (Type::Fn { .. }, _) => panic!(
            "Expected (generic, instance), got ({:?}, {:?})",
            generic, instance
        ),
        (
            Type::Tuple {
                elems: generic_elems,
            },
            Type::Tuple {
                elems: instance_elems,
            },
        ) if generic_elems.len() == instance_elems.len() => generic_elems
            .iter()
            .zip(instance_elems.iter())
            .for_each(|(g, i)| solve_type_apps(g.clone(), i.clone(), result)),
        (Type::Tuple { .. }, _) => panic!(
            "Expected (generic, instance), got ({:?}, {:?})",
            generic, instance
        ),
    }
}

pub(crate) fn is_type_public_and_transparent(self_module: &TypedModule, name: &EcoString) -> bool {
    self_module
        .definitions
        .iter()
        .find_map(|def| match def {
            Definition::Function(_function) => None,
            Definition::TypeAlias(_type_alias) => None,
            Definition::CustomType(custom_type) if &custom_type.name == name => {
                Some(custom_type.publicity.is_public() && !custom_type.opaque)
            }
            Definition::CustomType(_custom_type) => None,
            Definition::Import(_import) => None,
            Definition::ModuleConstant(_module_constant) => None,
        })
        .unwrap_or(true) // type is not local but the type checker has verified that it is public (and if constructors are accessed, those are public, too)
}

pub(crate) fn is_type_single_constructor(
    dep_modules: &im::HashMap<EcoString, ModuleInterface>,
    self_module: &TypedModule,
    module: &EcoString,
    name: &EcoString,
) -> bool {
    if module == &self_module.name {
        self_module
            .definitions
            .iter()
            .find_map(|def| match def {
                Definition::Function(_function) => None,
                Definition::TypeAlias(_type_alias) => None,
                Definition::CustomType(custom_type) if &custom_type.name == name => {
                    Some(custom_type.constructors.len() == 1)
                }
                Definition::CustomType(_custom_type) => None,
                Definition::Import(_import) => None,
                Definition::ModuleConstant(_module_constant) => None,
            })
            .expect("local type definition")
    } else {
        dep_modules
            .get(module)
            .expect("dependent module")
            .types_value_constructors
            .get(name)
            .expect("global type definition")
            .variants
            .len()
            == 1
    }
}

fn is_public_constructor(
    self_module: &TypedModule,
    module: Option<&EcoString>,
    name: &str,
) -> bool {
    if module.is_some() && module != Some(&self_module.name) {
        return true;
    }
    self_module.definitions.iter().any(|def| match def {
        Definition::Function(_function) => false,
        Definition::TypeAlias(_type_alias) => false,
        Definition::CustomType(custom_type) => {
            custom_type.publicity.is_public()
                && !custom_type.opaque
                && custom_type.constructors.iter().any(|con| con.name == name)
        }
        Definition::Import(import) => import.unqualified_values.iter().any(|v| v.name == name),
        Definition::ModuleConstant(_module_constant) => false,
    })
}

fn list_element_type(type_: &Type) -> Arc<Type> {
    let (type_module, type_name, type_args) = type_.named_type_information().expect("named type");
    if type_module != PRELUDE_MODULE_NAME || type_name != "List" {
        panic!("Expected List, got {:?}.{:?}", type_module, type_name);
    }
    if type_args.len() != 1 {
        panic!(
            "Expected List with one type argument, got {:?}",
            type_args.len()
        );
    }
    type_args[0].clone()
}

pub fn int(value: &str) -> Document<'_> {
    let mut out = EcoString::with_capacity(value.len());

    if value.starts_with('-') {
        out.push('-');
    } else if value.starts_with('+') {
        out.push('+');
    };
    let value = value.trim_start_matches(['+', '-'].as_ref());

    let value = if value.starts_with("0x") {
        out.push_str("0x");
        value.trim_start_matches("0x")
    } else if value.starts_with("0o") {
        out.push_str("0o");
        value.trim_start_matches("0o")
    } else if value.starts_with("0b") {
        out.push_str("0b");
        value.trim_start_matches("0b")
    } else {
        value
    };

    let value = value.trim_start_matches('0');
    if value.is_empty() {
        out.push('0');
    }
    out.push_str(value);

    out.to_doc()
}

pub fn float(value: &str) -> Document<'_> {
    let mut out = EcoString::with_capacity(value.len());

    if value.starts_with('-') {
        out.push('-');
    } else if value.starts_with('+') {
        out.push('+');
    };
    let value = value.trim_start_matches(['+', '-'].as_ref());

    let value = value.trim_start_matches('0');
    if value.starts_with(['.', 'e', 'E']) {
        out.push('0');
    }
    out.push_str(value);

    out.to_doc()
}

pub(crate) fn guard_constant_expression<'a>(
    dep_modules: &im::HashMap<EcoString, ModuleInterface>,
    self_module: &TypedModule,
    assignments: &mut Vec<Assignment<'a>>,
    tracker: &mut UsageTracker,
    expression: &'a TypedConstant,
    current_scope_vars: &im::HashMap<EcoString, (bool, usize)>,
    generic_type_ids_in_scope: &HashSet<u64>,
) -> Output<'a> {
    match expression {
        Constant::Tuple { elements, .. } => Ok(docvec![
            tuple_type(
                self_module,
                &elements.iter().map(|element| element.type_()).collect(),
                tracker,
                generic_type_ids_in_scope
            ),
            initializer(elements.iter().map(|e| {
                guard_constant_expression(
                    dep_modules,
                    self_module,
                    assignments,
                    tracker,
                    e,
                    current_scope_vars,
                    generic_type_ids_in_scope,
                )
            }))?,
        ]),
        Constant::List {
            elements, type_, ..
        } => {
            tracker.list_used = true;
            let elem_type = list_element_type(type_);
            let elem_type_doc =
                type_doc(self_module, &elem_type, tracker, generic_type_ids_in_scope);
            list(
                elements.iter().map(|e| {
                    guard_constant_expression(
                        dep_modules,
                        self_module,
                        assignments,
                        tracker,
                        e,
                        current_scope_vars,
                        generic_type_ids_in_scope,
                    )
                }),
                elem_type_doc,
            )
        }
        Constant::Record { type_, name, .. } if type_.is_bool() && name == "True" => {
            Ok("true".to_doc())
        }
        Constant::Record { type_, name, .. } if type_.is_bool() && name == "False" => {
            Ok("false".to_doc())
        }

        Constant::Record {
            args,
            module,
            name,
            tag,
            type_,
            ..
        } => {
            let mut module = module.as_ref().map(|(module, _)| module.clone());
            if type_.is_result() {
                if tag == "Ok" {
                    tracker.ok_used = true;
                } else {
                    tracker.error_used = true;
                }
                module = Some(PRELUDE_MODULE_NAME.into());
            } else if type_.is_nil() {
                tracker.nil_used = true;
                module = Some(PRELUDE_MODULE_NAME.into());
            }

            let (_, _, type_args) = match type_.fn_types() {
                Some((_, ret_type)) => ret_type.named_type_information().expect("named type"),
                None => type_.named_type_information().expect("named type"),
            };

            // If there's no arguments and the type is a function that takes
            // arguments then this is the constructor being referenced, not the
            // function being called.
            if let Some(arity) = type_.fn_arity() {
                if args.is_empty() && arity != 0 {
                    let arity = arity as u16;
                    return Ok(record_constructor(
                        self_module,
                        type_.clone(),
                        module,
                        name.clone(),
                        arity,
                        tracker,
                        generic_type_ids_in_scope,
                    ));
                }
            }

            let field_values: Vec<_> = args
                .iter()
                .map(|arg| {
                    guard_constant_expression(
                        dep_modules,
                        self_module,
                        assignments,
                        tracker,
                        &arg.value,
                        current_scope_vars,
                        generic_type_ids_in_scope,
                    )
                })
                .try_collect()?;
            Ok(construct_record(
                self_module,
                tracker,
                generic_type_ids_in_scope,
                module,
                name.to_owned(),
                &type_args,
                field_values,
            ))
        }

        Constant::BitArray { segments, .. } => bit_array(tracker, segments, |tracker, constant| {
            guard_constant_expression(
                dep_modules,
                self_module,
                assignments,
                tracker,
                constant,
                current_scope_vars,
                generic_type_ids_in_scope,
            )
        }),

        Constant::Var { name, .. } => Ok(assignments
            .iter()
            .find(|assignment| assignment.name == name)
            .map(|assignment| assignment.subject.clone().append(assignment.path.clone()))
            .unwrap_or_else(|| maybe_escape_identifier_doc(name))),

        expression => constant_expression(
            dep_modules,
            self_module,
            tracker,
            expression,
            current_scope_vars,
            generic_type_ids_in_scope,
        ),
    }
}

pub(crate) fn constant_expression<'a>(
    dep_modules: &im::HashMap<EcoString, ModuleInterface>,
    self_module: &TypedModule,
    tracker: &mut UsageTracker,
    expression: &'a TypedConstant,
    current_scope_vars: &im::HashMap<EcoString, (bool, usize)>,
    generic_type_ids_in_scope: &HashSet<u64>,
) -> Output<'a> {
    match expression {
        Constant::Int { value, .. } => Ok(int(value)),
        Constant::Float { value, .. } => Ok(float(value)),
        Constant::String { value, .. } => Ok(string(value)),
        Constant::Tuple { elements, .. } => Ok(docvec![
            tuple_type(
                self_module,
                &elements.iter().map(|element| element.type_()).collect(),
                tracker,
                generic_type_ids_in_scope
            ),
            initializer(elements.iter().map(|element| constant_expression(
                dep_modules,
                self_module,
                tracker,
                element,
                current_scope_vars,
                generic_type_ids_in_scope
            )),)?
        ]),
        Constant::List {
            elements, type_, ..
        } => {
            tracker.list_used = true;
            let elem_type = list_element_type(type_);
            let elem_type_doc =
                type_doc(self_module, &elem_type, tracker, generic_type_ids_in_scope);
            let list = list(
                elements.iter().map(|e| {
                    constant_expression(
                        dep_modules,
                        self_module,
                        tracker,
                        e,
                        current_scope_vars,
                        generic_type_ids_in_scope,
                    )
                }),
                elem_type_doc,
            )?;

            Ok(list)
        }

        Constant::Record { type_, name, .. } if type_.is_bool() && name == "True" => {
            Ok("true".to_doc())
        }
        Constant::Record { type_, name, .. } if type_.is_bool() && name == "False" => {
            Ok("false".to_doc())
        }

        Constant::Record {
            args,
            module,
            name,
            tag,
            type_,
            ..
        } => {
            let name = match module {
                Some(module) => (Some(module.0.clone()), name.clone()),
                None if type_.is_result() => {
                    if tag == "Ok" {
                        tracker.ok_used = true;
                        (Some(PRELUDE_MODULE_NAME.into()), name.clone())
                    } else {
                        tracker.error_used = true;
                        (Some(PRELUDE_MODULE_NAME.into()), name.clone())
                    }
                }
                None if type_.is_nil() => {
                    tracker.nil_used = true;
                    (Some(PRELUDE_MODULE_NAME.into()), name.clone())
                }
                None => self_module
                    .definitions
                    .iter()
                    .find_map(|def| match def {
                        Definition::Import(import) => {
                            import.unqualified_values.iter().find_map(|v| {
                                if v.used_name() == name {
                                    Some((Some(import.module.clone()), v.name.clone()))
                                } else {
                                    None
                                }
                            })
                        }
                        _ => None,
                    })
                    .unwrap_or((None, name.clone())),
            };

            // If there's no arguments and the type is a function that takes
            // arguments then this is the constructor being referenced, not the
            // function being called.
            if let Some(arity) = type_.fn_arity() {
                if args.is_empty() && arity != 0 {
                    let arity = arity as u16;
                    return Ok(record_constructor(
                        self_module,
                        type_.clone(),
                        name.0.clone(),
                        name.1.clone(),
                        arity,
                        tracker,
                        generic_type_ids_in_scope,
                    ));
                }
            }

            let type_args = match type_.fn_types() {
                Some((_, ret_type)) => ret_type.named_type_information().expect("named type").2,
                None => type_.named_type_information().expect("named type").2,
            };

            let field_values: Vec<_> = args
                .iter()
                .map(|arg| {
                    constant_expression(
                        dep_modules,
                        self_module,
                        tracker,
                        &arg.value,
                        current_scope_vars,
                        generic_type_ids_in_scope,
                    )
                })
                .try_collect()?;

            let constructor = construct_record(
                self_module,
                tracker,
                generic_type_ids_in_scope,
                name.0,
                name.1,
                &type_args,
                field_values,
            );
            Ok(constructor)
        }

        Constant::BitArray { segments, .. } => {
            let bit_array = bit_array(tracker, segments, |tracker, expr| {
                constant_expression(
                    dep_modules,
                    self_module,
                    tracker,
                    expr,
                    current_scope_vars,
                    generic_type_ids_in_scope,
                )
            })?;
            Ok(bit_array)
        }

        Constant::Var {
            name,
            module,
            type_,
            ..
        } => {
            let (module, name, generic_type) = match module {
                Some((module, _)) => {
                    let orig_module = self_module
                        .definitions
                        .iter()
                        .find_map(|def| match def {
                            Definition::Import(import)
                                if import.used_name().as_ref() == Some(module) =>
                            {
                                Some(import.module.clone())
                            }
                            _ => None,
                        })
                        .expect("module imported");
                    (
                        Some(module.clone()),
                        name.clone(),
                        dep_modules
                            .get(&orig_module)
                            .expect("dependent module")
                            .get_public_value(name)
                            .expect("value exists")
                            .type_
                            .clone(),
                    )
                }
                None => self_module
                    .definitions
                    .iter()
                    .find_map(|def| match def {
                        Definition::Function(function) => match &function.name {
                            Some((_, fname)) if fname == name => {
                                let args = function.arguments.iter().map(|arg| arg.type_.clone());
                                Some((
                                    None,
                                    name.clone(),
                                    Arc::new(Type::Fn {
                                        args: args.collect(),
                                        retrn: function.return_type.clone(),
                                    }),
                                ))
                            }
                            _ => None,
                        },
                        Definition::ModuleConstant(module_constant)
                            if &module_constant.name == name =>
                        {
                            Some((None, name.clone(), module_constant.type_.clone()))
                        }
                        Definition::Import(import) => {
                            match import
                                .unqualified_values
                                .iter()
                                .find(|v| v.used_name() == name)
                            {
                                None => None,
                                Some(v) => Some((
                                    Some(import.module.clone()),
                                    v.name.clone(),
                                    dep_modules
                                        .get(&import.module)
                                        .expect("dependent module")
                                        .get_public_value(&v.name)
                                        .expect("value exists")
                                        .type_
                                        .clone(),
                                )),
                            }
                        }
                        _ => None,
                    })
                    .unwrap(),
            };

            let mut id_map = im::HashMap::new();
            solve_type_apps(generic_type, type_.clone(), &mut id_map);
            let type_args = id_map
                .iter()
                .sorted_by_key(|(k, _)| *k)
                .map(|(_, v)| type_doc(self_module, &v, tracker, generic_type_ids_in_scope))
                .collect::<Vec<_>>();

            let type_args_doc = if type_args.is_empty() {
                nil()
            } else {
                wrap_generic_args(type_args)
            };

            match module {
                None => {
                    let public = current_scope_vars
                        .get(&name)
                        .map_or(false, |&(public, _)| public);
                    Ok(docvec![to_go_name(&name, public), type_args_doc])
                }
                Some(module) => Ok(docvec![
                    to_go_package_name(&module),
                    ".",
                    to_go_name(&name, true),
                    type_args_doc
                ]),
            }
        }
        Constant::StringConcatenation { left, right, .. } => {
            let left = constant_expression(
                dep_modules,
                self_module,
                tracker,
                left,
                current_scope_vars,
                generic_type_ids_in_scope,
            )?;
            let right = constant_expression(
                dep_modules,
                self_module,
                tracker,
                right,
                current_scope_vars,
                generic_type_ids_in_scope,
            )?;
            Ok(docvec![left, " + ", right])
        }

        Constant::Invalid { .. } => panic!("invalid constants should not reach code generation"),
    }
}

fn bit_array<'a>(
    tracker: &mut UsageTracker,
    segments: &'a [TypedConstantBitArraySegment],
    mut constant_expr_fun: impl FnMut(&mut UsageTracker, &'a TypedConstant) -> Output<'a>,
) -> Output<'a> {
    tracker.bit_array_literal_used = true;

    use BitArrayOption as Opt;

    let segments_array = comma_separated_list(segments.iter().map(|segment| {
        let value = constant_expr_fun(tracker, &segment.value)?;

        if segment.type_ == crate::type_::int() || segment.type_ == crate::type_::float() {
            let details =
                sized_bit_array_segment_details(segment, tracker, &mut constant_expr_fun)?;

            if segment.type_ == crate::type_::int() {
                match (details.size_value, segment.value.as_ref()) {
                    (Some(size_value), Constant::Int { int_value, .. })
                        if size_value <= SAFE_INT_SEGMENT_MAX_SIZE.into() =>
                    {
                        let bytes = bit_array_segment_int_value_to_bytes(
                            int_value.clone(),
                            size_value,
                            details.endianness,
                        )?;

                        Ok(u8_slice(&bytes))
                    }

                    (Some(size_value), _) if size_value == 8.into() => Ok(value),

                    (Some(size_value), _) if size_value <= 0.into() => Ok(nil()),

                    _ => {
                        tracker.sized_integer_segment_used = true;
                        Ok(docvec![
                            to_go_package_name(PRELUDE_MODULE_NAME),
                            ".SizedInt(",
                            value,
                            ", ",
                            details.size,
                            ", ",
                            bool(details.endianness.is_big()),
                            ")"
                        ])
                    }
                }
            } else {
                tracker.float_bit_array_segment_used = true;
                Ok(docvec![
                    to_go_package_name(PRELUDE_MODULE_NAME),
                    ".SizedFloat(",
                    value,
                    ", ",
                    details.size,
                    ", ",
                    bool(details.endianness.is_big()),
                    ")"
                ])
            }
        } else {
            match segment.options.as_slice() {
                // UTF8 strings
                [Opt::Utf8 { .. }] => {
                    tracker.string_bit_array_segment_used = true;
                    Ok(docvec![
                        to_go_package_name(PRELUDE_MODULE_NAME),
                        ".StringBits(",
                        value,
                        ")"
                    ])
                }

                // UTF8 codepoints
                [Opt::Utf8Codepoint { .. }] => {
                    tracker.codepoint_bit_array_segment_used = true;
                    Ok(docvec![
                        to_go_package_name(PRELUDE_MODULE_NAME),
                        ".CodepointBits(",
                        value,
                        ")"
                    ])
                }

                // Bit strings
                [Opt::Bits { .. }] => Ok(docvec![value, ".Buffer()"]),

                // Anything else
                _ => Err(Error::Unsupported {
                    feature: "This bit array segment option".into(),
                    location: segment.location,
                }),
            }
        }
    }))?;

    Ok(docvec![
        to_go_package_name(PRELUDE_MODULE_NAME),
        ".ToBitArray(",
        segments_array,
        ")"
    ])
}

#[derive(Debug)]
struct SizedBitArraySegmentDetails<'a> {
    size: Document<'a>,
    /// The size of the bit array segment stored as a BigInt. This has a value when the segment's
    /// size is known at compile time.
    size_value: Option<BigInt>,
    endianness: Endianness,
}

fn sized_bit_array_segment_details<'a>(
    segment: &'a TypedConstantBitArraySegment,
    tracker: &mut UsageTracker,
    constant_expr_fun: &mut impl FnMut(&mut UsageTracker, &'a TypedConstant) -> Output<'a>,
) -> Result<SizedBitArraySegmentDetails<'a>, Error> {
    use BitArrayOption as Opt;

    if segment
        .options
        .iter()
        .any(|x| matches!(x, Opt::Native { .. }))
    {
        return Err(Error::Unsupported {
            feature: "This bit array segment option".into(),
            location: segment.location,
        });
    }

    let endianness = if segment
        .options
        .iter()
        .any(|x| matches!(x, Opt::Little { .. }))
    {
        Endianness::Little
    } else {
        Endianness::Big
    };

    let size = segment
        .options
        .iter()
        .find(|x| matches!(x, Opt::Size { .. }));

    let (size_value, size) = match size {
        Some(Opt::Size { value: size, .. }) => {
            let size_value = match *size.clone() {
                Constant::Int { int_value, .. } => Some(int_value),
                _ => None,
            };

            if let Some(size_value) = size_value.as_ref() {
                if *size_value > BigInt::ZERO && size_value % 8 != BigInt::ZERO {
                    return Err(Error::Unsupported {
                        feature: "Non byte aligned array".into(),
                        location: segment.location,
                    });
                }
            }

            (size_value, constant_expr_fun(tracker, size)?)
        }
        _ => {
            let size_value = if segment.type_ == crate::type_::int() {
                8usize
            } else {
                64usize
            };

            (Some(BigInt::from(size_value)), docvec![size_value])
        }
    };

    Ok(SizedBitArraySegmentDetails {
        size,
        size_value,
        endianness,
    })
}

static UNICODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\\u\{([0-9a-fA-F]{1,6})\}"#).expect("valid regex"));

pub fn string(value: &str) -> Document<'_> {
    let to_go_unicode = |caps: &regex::Captures<'_>| {
        let digits = caps.get(1).unwrap();
        format!("\\U{:0>8}", digits.as_str())
    };
    let with_go_unicode = UNICODE_RE.replace_all(value, &to_go_unicode);
    EcoString::from(with_go_unicode)
        .to_doc()
        .surround("\"", "\"")

    // if value.contains('\n') {
    //     EcoString::from(value.replace('\n', r"\n"))
    //         .to_doc()
    //         .surround("\"", "\"")
    // } else {
    //     value.to_doc().surround("\"", "\"")
    // }
}

pub fn comma_separated_list<'a, Elements: IntoIterator<Item = Output<'a>>>(
    elements: Elements,
) -> Output<'a> {
    let elements = Itertools::intersperse(elements.into_iter(), Ok(break_(",", ", ")))
        .collect::<Result<Vec<_>, _>>()?;
    if elements.is_empty() {
        Ok(nil())
    } else {
        Ok(docvec![
            docvec![break_("", ""), elements].nest(INDENT),
            break_(",", ""),
        ]
        .group())
    }
}

pub fn initializer<'a, Elements: IntoIterator<Item = Output<'a>>>(
    elements: Elements,
) -> Output<'a> {
    Ok(docvec!["{", comma_separated_list(elements)?, "}"])
}

fn list<'a, I: IntoIterator<Item = Output<'a>>>(
    elements: I,
    element_type: Document<'a>,
) -> Output<'a>
where
    I::IntoIter: DoubleEndedIterator + ExactSizeIterator,
{
    Ok(docvec![
        to_go_package_name(PRELUDE_MODULE_NAME),
        ".ToList[",
        element_type,
        "](",
        comma_separated_list(elements)?,
        ")"
    ])
}

fn prepend<'a, I: IntoIterator<Item = Output<'a>>>(
    elements: I,
    elem_type: Document<'a>,
    tail: Document<'a>,
) -> Output<'a>
where
    I::IntoIter: DoubleEndedIterator + ExactSizeIterator,
{
    elements.into_iter().rev().try_fold(tail, |tail, element| {
        let args = call_arguments([element, Ok(tail)])?;
        Ok(docvec![
            to_go_package_name(PRELUDE_MODULE_NAME),
            ".ListPrepend[",
            elem_type.clone(),
            "]",
            args
        ])
    })
}

fn call_arguments<'a, Elements: IntoIterator<Item = Output<'a>>>(elements: Elements) -> Output<'a> {
    let elements = Itertools::intersperse(elements.into_iter(), Ok(break_(",", ", ")))
        .collect::<Result<Vec<_>, _>>()?
        .to_doc();
    if elements.is_empty() {
        return Ok("()".to_doc());
    }
    Ok(docvec![
        "(",
        docvec![break_("", ""), elements].nest(INDENT),
        break_(",", ""),
        ")"
    ]
    .group())
}

fn construct_record<'a>(
    self_module: &TypedModule,
    tracker: &mut UsageTracker,
    generic_ids_in_scope: &HashSet<u64>,
    module: Option<EcoString>,
    name: EcoString,
    type_args: &[Arc<Type>],
    arguments: impl IntoIterator<Item = Document<'a>>,
) -> Document<'a> {
    let mut any_arguments = false;
    let arguments = join(
        arguments.into_iter().inspect(|_| {
            any_arguments = true;
        }),
        break_(",", ", "),
    );
    let arguments = docvec![break_("", ""), arguments].nest(INDENT);

    let (module, name) = self_module
        .definitions
        .iter()
        .find_map(|def| match def {
            Definition::Import(import) => import.unqualified_values.iter().find_map(|v| {
                if v.used_name() == &name {
                    Some((import.used_name(), v.name.clone()))
                } else {
                    None
                }
            }),
            _ => None,
        })
        .unwrap_or((module, name.into()));

    let public = is_public_constructor(self_module, module.as_ref(), &name);
    let name = if let Some(module) = module {
        if module != self_module.name {
            docvec![
                to_go_package_name(&module),
                ".",
                to_go_constructor_name(&name, public).to_doc()
            ]
        } else {
            to_go_constructor_name(&name, public).to_doc()
        }
    } else {
        to_go_constructor_name(&name, public).to_doc()
    };

    let type_args_doc = if type_args.len() > 0 {
        wrap_generic_args(
            type_args
                .iter()
                .map(|a| type_doc(self_module, a, tracker, generic_ids_in_scope)),
        )
    } else {
        nil()
    };
    if any_arguments {
        docvec![name, type_args_doc, "{", arguments, break_(",", ""), "}"].group()
    } else {
        docvec![name, type_args_doc, "{}"]
    }
}

impl TypedExpr {
    fn handles_own_return_go(&self) -> bool {
        match self {
            TypedExpr::Todo { .. }
            | TypedExpr::Call { .. }
            | TypedExpr::Case { .. }
            | TypedExpr::Panic { .. }
            | TypedExpr::Block { .. }
            | TypedExpr::Pipeline { .. }
            | TypedExpr::RecordUpdate { .. } => true,

            TypedExpr::Int { .. }
            | TypedExpr::Float { .. }
            | TypedExpr::String { .. }
            | TypedExpr::Var { .. }
            | TypedExpr::Fn { .. }
            | TypedExpr::List { .. }
            | TypedExpr::BinOp { .. }
            | TypedExpr::RecordAccess { .. }
            | TypedExpr::ModuleSelect { .. }
            | TypedExpr::Tuple { .. }
            | TypedExpr::TupleIndex { .. }
            | TypedExpr::BitArray { .. }
            | TypedExpr::NegateBool { .. }
            | TypedExpr::NegateInt { .. }
            | TypedExpr::Invalid { .. } => false,
        }
    }
}

impl BinOp {
    fn is_operator_to_wrap_go(&self) -> bool {
        match self {
            BinOp::And
            | BinOp::Or
            | BinOp::Eq
            | BinOp::NotEq
            | BinOp::LtInt
            | BinOp::LtEqInt
            | BinOp::LtFloat
            | BinOp::LtEqFloat
            | BinOp::GtEqInt
            | BinOp::GtInt
            | BinOp::GtEqFloat
            | BinOp::GtFloat
            | BinOp::AddInt
            | BinOp::AddFloat
            | BinOp::SubInt
            | BinOp::SubFloat
            | BinOp::MultFloat
            | BinOp::DivInt
            | BinOp::DivFloat
            | BinOp::RemainderInt
            | BinOp::Concatenate => true,
            BinOp::MultInt => false,
        }
    }
}

pub fn is_go_scalar(t: Arc<Type>) -> bool {
    t.is_int() || t.is_float() || t.is_bool() || t.is_string()
}

/// Wrap a document in an immediately involked function expression
fn immediately_invoked_function_expression_document<'a, 'b>(
    self_module: &TypedModule,
    document: Document<'a>,
    type_: Arc<Type>,
    tracker: &mut UsageTracker,
    generic_type_ids_in_scope: &'b HashSet<u64>,
) -> Document<'a> {
    docvec![
        docvec![
            "(func() ",
            type_doc(self_module, &type_, tracker, generic_type_ids_in_scope),
            " {",
            break_("", " "),
            document
        ]
        .nest(INDENT),
        break_("", " "),
        "})()",
    ]
    .group()
}

fn record_constructor<'a>(
    self_module: &TypedModule,
    type_: Arc<Type>,
    mut qualifier: Option<EcoString>,
    name: EcoString,
    arity: u16,
    tracker: &mut UsageTracker,
    generic_ids_in_scope: &HashSet<u64>,
) -> Document<'a> {
    if type_.is_bool() {
        return if name == "True" {
            "true".to_doc()
        } else {
            "false".to_doc()
        };
    }

    if qualifier.is_none() || qualifier == Some(PRELUDE_MODULE_NAME.into()) {
        if type_.is_result_constructor() {
            if name == "Ok" {
                tracker.ok_used = true;
                qualifier = Some(PRELUDE_MODULE_NAME.into());
            } else if name == "Error" {
                tracker.error_used = true;
                qualifier = Some(PRELUDE_MODULE_NAME.into());
            }
        } else if type_.is_nil() {
            tracker.nil_used = true;
            qualifier = Some(PRELUDE_MODULE_NAME.into());
        }
    }

    let public = qualifier.as_ref().is_some()
        || is_public_constructor(
            self_module,
            qualifier.as_ref().map(|q| EcoString::from(q)).as_ref(),
            &name,
        );
    if arity == 0 {
        let name = self_module
            .definitions
            .iter()
            .find_map(|def| match def {
                Definition::Import(import) => import.unqualified_values.iter().find_map(|v| {
                    if v.used_name() == &name {
                        Some(v.name.clone())
                    } else {
                        None
                    }
                }),
                _ => None,
            })
            .unwrap_or(name.into());

        let type_args = type_.named_type_information().expect("named type").2;
        let type_args_doc = if type_args.is_empty() {
            nil()
        } else {
            wrap_generic_args(
                type_args
                    .iter()
                    .map(|a| type_doc(self_module, a, tracker, generic_ids_in_scope)),
            )
        };

        match qualifier {
            Some(module) => docvec![
                to_go_package_name(&module),
                ".",
                to_go_constructor_name(&name, public),
                type_args_doc,
                "{}"
            ],
            None => docvec![to_go_constructor_name(&name, public), type_args_doc, "{}"],
        }
    } else {
        let (arg_types, ret_type) = type_.fn_types().expect("function type");
        let params = (0..arity)
            .map(|i| {
                docvec![
                    to_go_positional_field_name(i.try_into().unwrap(), public),
                    " ",
                    type_doc(
                        self_module,
                        &arg_types[i as usize],
                        tracker,
                        generic_ids_in_scope
                    )
                ]
            })
            .collect::<Vec<_>>();
        let args = (0..arity)
            .map(|i| to_go_positional_field_name(i.try_into().unwrap(), public).to_doc())
            .collect::<Vec<_>>();
        let (_, _, type_args) = ret_type.named_type_information().expect("named type");
        let body = docvec![
            "return ",
            construct_record(
                self_module,
                tracker,
                generic_ids_in_scope,
                qualifier.map(|q| EcoString::from(q)),
                name,
                &type_args,
                args.clone()
            )
        ];
        docvec![
            docvec![
                "func",
                wrap_args(params),
                " ",
                type_doc(self_module, &ret_type, tracker, generic_ids_in_scope),
                " {",
                break_("", " "),
                body
            ]
            .nest(INDENT)
            .append(break_("", " "))
            .group(),
            "}",
        ]
    }
}

fn u8_slice<'a>(bytes: &[u8]) -> Document<'a> {
    let s: EcoString = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ")
        .into();

    docvec!["[]byte{", s, "}"]
}
