// TODO: remove
#![allow(unused)]

//! An implementation of the algorithm described at
//! <https://julesjacobs.com/notes/patternmatching/patternmatching.pdf>.
//!
//! Adapted from Yorick Peterse's implementation at
//! <https://github.com/yorickpeterse/pattern-matching-in-rust>. Thank you Yorick!
//!
//! Note that while this produces a decision tree, this tree is not suitable for
//! use in code generation yet as it is incomplete. The tree is not correctly
//! formed for:
//! - Bit strings
//! - String prefixes
//!
//! These were not implemented as they are more complex and I've not worked out
//! a good way to do them yet. The tricky bit is that unlike the others they are
//! not an exact match and they can overlap with other patterns. Take this
//! example:
//!
//! ```text
//! case x {
//!    "1" <> _ -> ...
//!    "12" <> _ -> ...
//!    "123" -> ...
//!    _ -> ...
//! }
//! ```
//!
//! The decision tree needs to take into account that the first pattern is a
//! super-pattern of the second, and the second is a super-pattern of the third.
//!

mod missing_patterns;
mod pattern;
#[cfg(test)]
mod pattern_tests;
#[cfg(test)]
mod tests;

use self::pattern::{Constructor, Pattern, PatternId};
use crate::{
    ast::AssignName,
    type_::{
        collapse_links, environment, error::UnknownTypeConstructorError, is_prelude_module,
        Environment, ModuleInterface, Type, TypeValueConstructor, TypeValueConstructorParameter,
        TypeVar,
    },
};
use ecow::EcoString;
use id_arena::Arena;
use itertools::Itertools;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub use self::pattern::PatternArena;

/// The body of code to evaluate in case of a match.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Body {
    /// Any variables to bind before running the code.
    ///
    /// The tuples are in the form `(name, source)` (i.e `bla = source`).
    bindings: Vec<(EcoString, Variable)>,

    /// The index of the clause in the case expression that should be run.
    clause_index: u16,
}

impl Body {
    pub fn new(clause_index: u16) -> Self {
        Self {
            bindings: vec![],
            clause_index,
        }
    }
}

/// A variable used in a match expression.
///
/// In a real compiler these would probably be registers or some other kind of
/// variable/temporary generated by your compiler.
#[derive(Eq, PartialEq, Clone, Debug)]
pub struct Variable {
    id: usize,
    type_: Arc<Type>,
}

/// A single case (or row) in a match expression/table.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Row {
    columns: Vec<Column>,
    guard: Option<usize>,
    body: Body,
}

impl Row {
    pub fn new(columns: Vec<Column>, guard: Option<usize>, body: Body) -> Self {
        Self {
            columns,
            guard,
            body,
        }
    }

    fn remove_column(&mut self, variable_id: usize) -> Option<Column> {
        self.columns
            .iter()
            .position(|c| c.variable.id == variable_id)
            .map(|idx| self.columns.remove(idx))
    }
}

/// A column in a pattern matching table.
///
/// A column contains a single variable to test, and a pattern to test against
/// that variable. A row may contain multiple columns, though this wouldn't be
/// exposed to the source language (= it's an implementation detail).
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Column {
    variable: Variable,
    pattern: PatternId,
}

impl Column {
    pub fn new(variable: Variable, pattern: PatternId) -> Self {
        Self { variable, pattern }
    }
}

/// A case in a decision tree to test against a variable.
#[derive(Eq, PartialEq, Debug)]
pub struct Case {
    /// The constructor to test against an input variable.
    constructor: Constructor,

    /// Variables to introduce to the body of this case.
    ///
    /// At runtime these would be populated with the values a pattern is matched
    /// against. For example, this pattern:
    ///
    /// ```text
    /// case (10, 20, one) -> ...
    /// ```
    ///
    /// Would result in three arguments, assigned the values `10`, `20` and
    /// `one`.
    ///
    /// In a real compiler you'd assign these variables in your IR first, then
    /// generate the code for the sub tree.
    arguments: Vec<Variable>,

    /// The sub tree of this case.
    body: Decision,
}

impl Case {
    fn new(constructor: Constructor, arguments: Vec<Variable>, body: Decision) -> Self {
        Self {
            constructor,
            arguments,
            body,
        }
    }
}

/// A decision tree
#[derive(Eq, PartialEq, Debug)]
pub enum Decision {
    /// A pattern is matched and the right-hand value is to be returned.
    Success(Body),

    /// A pattern is missing.
    Failure,

    /// Checks if a guard evaluates to true, running the body if it does.
    ///
    /// The arguments are as follows:
    ///
    /// 1. The "condition" to evaluate. We just use a dummy value, but in a real
    ///    compiler this would likely be an AST node of sorts.
    /// 2. The body to evaluate if the guard matches.
    /// 3. The sub tree to evaluate when the guard fails.
    Guard(usize, Body, Box<Decision>),

    /// Checks if a value is any of the given patterns.
    ///
    /// The values are as follows:
    ///
    /// 1. The variable to test.
    /// 2. The cases to test against this variable.
    /// 3. A fallback decision to take, in case none of the cases matched.
    Switch(Variable, Vec<Case>, Option<Box<Decision>>),

    /// Checks if a list is empty or non-empty.
    List {
        /// The variable to test.
        variable: Variable,
        /// The decision to take if the list is empty.
        empty: Box<Decision>,
        /// The decision to take if the list is non-empty.
        non_empty: Box<NonEmptyListDecision>,
    },
}

#[derive(Eq, PartialEq, Debug)]
pub struct NonEmptyListDecision {
    first: Variable,
    rest: Variable,
    decision: Decision,
}

/// A type for storing diagnostics produced by the decision tree compiler.
#[derive(Debug)]
pub struct Diagnostics {
    /// A flag indicating the match is missing one or more pattern.
    pub missing: bool,

    /// The right-hand sides that are reachable.
    ///
    /// If a right-hand side isn't in this list it means its pattern is
    /// redundant.
    pub reachable: Vec<u16>,
}

/// The result of compiling a pattern match expression.
pub struct Match {
    pub tree: Decision,
    pub diagnostics: Diagnostics,
}

impl Match {
    pub fn missing_patterns(&self, environment: &Environment<'_>) -> Vec<EcoString> {
        missing_patterns::missing_patterns(self, environment)
    }
}

/// The `match` compiler itself (shocking, I know).
#[derive(Debug)]
pub struct Compiler<'a> {
    variable_id: usize,
    diagnostics: Diagnostics,
    patterns: Arena<Pattern>,
    environment: &'a Environment<'a>,
}

impl<'a> Compiler<'a> {
    pub fn new(environment: &'a Environment<'a>, patterns: Arena<Pattern>) -> Self {
        Self {
            environment,
            patterns,
            variable_id: 0,
            diagnostics: Diagnostics {
                missing: false,
                reachable: Vec::new(),
            },
        }
    }

    pub fn compile(mut self, rows: Vec<Row>) -> Match {
        Match {
            tree: self.compile_rows(rows),
            diagnostics: self.diagnostics,
        }
    }

    pub fn set_pattern_arena(&mut self, arena: Arena<Pattern>) {
        self.patterns = arena;
    }

    fn pattern(&self, id: PatternId) -> &Pattern {
        self.patterns.get(id).expect("Unknown pattern id")
    }

    fn flatten_or(&self, id: PatternId, row: Row) -> Vec<(PatternId, Row)> {
        match self.pattern(id) {
            Pattern::Or { left, right } => vec![(*left, row.clone()), (*right, row)],

            Pattern::Int { .. }
            | Pattern::List { .. }
            | Pattern::Tuple { .. }
            | Pattern::Float { .. }
            | Pattern::Assign { .. }
            | Pattern::String { .. }
            | Pattern::Discard
            | Pattern::Variable { .. }
            | Pattern::EmptyList
            | Pattern::BitArray { .. }
            | Pattern::Constructor { .. }
            | Pattern::StringPrefix { .. } => vec![(id, row)],
        }
    }

    fn compile_rows(&mut self, rows: Vec<Row>) -> Decision {
        if rows.is_empty() {
            self.diagnostics.missing = true;
            return Decision::Failure;
        }

        let mut rows = rows
            .into_iter()
            .map(|row| self.move_unconditional_patterns(row))
            .collect::<Vec<_>>();

        // There may be multiple rows, but if the first one has no patterns
        // those extra rows are redundant, as a row without columns/patterns
        // always matches.
        let first_row_always_matches = rows.first().map(|c| c.columns.is_empty()).unwrap_or(false);
        if first_row_always_matches {
            let row = rows.remove(0);
            self.diagnostics.reachable.push(row.body.clause_index);

            return match row.guard {
                Some(guard) => Decision::Guard(guard, row.body, Box::new(self.compile_rows(rows))),
                None => Decision::Success(row.body),
            };
        }

        match self.branch_mode(&rows) {
            BranchMode::Infinite { variable } => {
                let (cases, fallback) = self.compile_infinite_cases(rows, variable.clone());
                Decision::Switch(variable, cases, Some(fallback))
            }

            BranchMode::Tuple { variable, types } => {
                let variables = self.new_variables(&types);
                let cases = vec![(Constructor::Tuple(types), variables, Vec::new())];
                let cases = self.compile_constructor_cases(rows, variable.clone(), cases);
                Decision::Switch(variable, cases, None)
            }

            BranchMode::List {
                variable,
                element_type,
            } => self.compile_list_cases(rows, variable, element_type),

            BranchMode::NamedType {
                variable,
                constructors: variants,
            } => {
                let cases = variants
                    .iter()
                    .enumerate()
                    .map(|(idx, constructor)| {
                        let variant = Constructor::Variant {
                            type_: variable.type_.clone(),
                            index: idx as u16,
                        };
                        let new_variables = self.constructor_parameter_variables(
                            &variable.type_,
                            &constructor.parameters,
                        );
                        (variant, new_variables, Vec::new())
                    })
                    .collect();
                let cases = self.compile_constructor_cases(rows, variable.clone(), cases);
                Decision::Switch(variable, cases, None)
            }
        }
    }

    /// String, ints and floats have an infinite number of constructors, so we
    /// specialise the compilation of their patterns with this function.
    fn compile_infinite_cases(
        &mut self,
        rows: Vec<Row>,
        branch_var: Variable,
    ) -> (Vec<Case>, Box<Decision>) {
        let mut raw_cases: Vec<(Constructor, Vec<Variable>, Vec<Row>)> = Vec::new();
        let mut fallback_rows = Vec::new();
        let mut tested: HashMap<TestKey, usize> = HashMap::new();

        #[derive(Debug, PartialEq, Eq, Hash)]
        enum TestKey {
            Prefix(EcoString),
            Exact(EcoString),
        }

        let branch_variable_id = branch_var.id;
        for mut row in rows {
            let col = match row.remove_column(branch_variable_id) {
                // This row does not match on the branch variable, so we push it
                // into the fallback rows to be tested later.
                None => {
                    fallback_rows.push(row);
                    continue;
                }
                // This row does match on the branch variable.
                Some(col) => col,
            };

            for (pattern, mut row) in self.flatten_or(col.pattern, row) {
                let (key, constructor) = match self.pattern(pattern) {
                    Pattern::Int { value } => (
                        TestKey::Exact(value.clone()),
                        Constructor::Int(value.clone()),
                    ),

                    Pattern::Float { value } => (
                        TestKey::Exact(value.clone()),
                        Constructor::Float(value.clone()),
                    ),

                    Pattern::BitArray { value } => {
                        (TestKey::Exact(value.clone()), Constructor::BitArray)
                    }

                    Pattern::String { value } => (
                        TestKey::Exact(value.clone()),
                        Constructor::String(value.clone()),
                    ),

                    Pattern::StringPrefix { prefix, rest } => {
                        let prefix = prefix.clone();
                        match rest {
                            AssignName::Discard(_) => (),
                            AssignName::Variable(name) => {
                                let name = name.clone();
                                let variable = Pattern::Variable { name: name.clone() };
                                let pattern = self.patterns.alloc(variable);
                                row.columns.push(Column::new(branch_var.clone(), pattern));
                            }
                        };
                        (TestKey::Prefix(prefix.clone()), Constructor::StringPrefix)
                    }

                    pattern @ (Pattern::Constructor { .. }
                    | Pattern::Assign { .. }
                    | Pattern::Tuple { .. }
                    | Pattern::Variable { .. }
                    | Pattern::Discard
                    | Pattern::EmptyList
                    | Pattern::List { .. }
                    | Pattern::Or { .. }) => panic!("Unexpected pattern {:?}", pattern),
                };

                match tested.get(&key) {
                    // This value has already been tested, so this is a redundant test.
                    // Add the row to the previous test rather than duplicating it.
                    Some(index) => {
                        let case = raw_cases.get_mut(*index).expect("Case must exist");
                        case.2.push(row);
                    }
                    // This is the first time testing the tag, so we add a case for it.
                    None => {
                        _ = tested.insert(key, raw_cases.len());
                        raw_cases.push((constructor, Vec::new(), vec![row]));
                    }
                }
            }
        }

        for (_, _, rows) in &mut raw_cases {
            rows.append(&mut fallback_rows.clone());
        }

        let cases = raw_cases
            .into_iter()
            .map(|(cons, vars, rows)| Case::new(cons, vars, self.compile_rows(rows)))
            .collect();

        (cases, Box::new(self.compile_rows(fallback_rows)))
    }

    /// Compiles the cases and sub cases for the constructor located at the
    /// column of the branching variable.
    ///
    /// What exactly this method does may be a bit hard to understand from the
    /// code, as there's simply quite a bit going on. Roughly speaking, it does
    /// the following:
    ///
    /// 1. It takes the column we're branching on (based on the branching
    ///    variable) and removes it from every row.
    /// 2. We add additional columns to this row, if the constructor takes any
    ///    arguments (which we'll handle in a nested match).
    /// 3. We turn the resulting list of rows into a list of cases, then compile
    ///    those into decision (sub) trees.
    ///
    /// If a row didn't include the branching variable, we copy that row into
    /// the list of rows for every constructor to test.
    ///
    /// For this to work, the `cases` variable must be prepared such that it has
    /// a triple for every constructor we need to handle. For an ADT with 10
    /// constructors, that means 10 triples. This is needed so this method can
    /// assign the correct sub matches to these constructors.
    ///
    /// Types with infinite constructors (e.g. int and string) are handled
    /// separately; they don't need most of this work anyway.
    fn compile_constructor_cases(
        &mut self,
        rows: Vec<Row>,
        branch_var: Variable,
        mut cases: Vec<(Constructor, Vec<Variable>, Vec<Row>)>,
    ) -> Vec<Case> {
        for mut row in rows {
            let column = match row.remove_column(branch_var.id) {
                // This row had the branching variable, so we compile it below.
                Some(column) => column,

                // This row didn't have the branching variable, meaning it does
                // not match on this constructor. In this case we copy the row
                // into each of the other cases.
                None => {
                    for (_, _, other_case_rows) in &mut cases {
                        other_case_rows.push(row.clone());
                    }
                    continue;
                }
            };

            for (pattern, row) in self.flatten_or(column.pattern, row) {
                // We should only be able to reach constructors here for well
                // typed code. Invalid patterns should have been caught by
                // earlier analysis.
                let (index, args) = match self.pattern(pattern) {
                    Pattern::Constructor {
                        constructor,
                        arguments,
                    } => (constructor.index(), arguments.clone()),

                    Pattern::Tuple { elements } => (0, elements.clone()),

                    pattern @ (Pattern::Or { .. }
                    | Pattern::Int { .. }
                    | Pattern::List { .. }
                    | Pattern::Float { .. }
                    | Pattern::Tuple { .. }
                    | Pattern::String { .. }
                    | Pattern::Assign { .. }
                    | Pattern::Discard
                    | Pattern::Variable { .. }
                    | Pattern::BitArray { .. }
                    | Pattern::EmptyList
                    | Pattern::StringPrefix { .. }) => panic!("Unexpected pattern {:?}", pattern),
                };

                let mut columns = row.columns;

                let case = cases.get_mut(index as usize).expect("Case must exist");
                for (var, pattern) in case.1.iter().zip(args.into_iter()) {
                    columns.push(Column::new(var.clone(), pattern));
                }
                case.2.push(Row::new(columns, row.guard, row.body));
            }
        }

        cases
            .into_iter()
            .map(|(cons, vars, rows)| Case::new(cons, vars, self.compile_rows(rows)))
            .collect()
    }

    fn compile_list_cases(
        &mut self,
        rows: Vec<Row>,
        branch_var: Variable,
        element_type: Arc<Type>,
    ) -> Decision {
        let mut empty_rows = vec![];
        let mut non_empty_rows = vec![];
        let first_var = self.new_variable(element_type);
        let rest_var = self.new_variable(branch_var.type_.clone());

        for mut row in rows {
            let column = match row.remove_column(branch_var.id) {
                // This row had the branching variable, so we compile it below.
                Some(column) => column,

                // This row didn't have the branching variable, meaning it does
                // not match on this constructor. In this case we copy the row
                // into each of the other cases.
                None => {
                    empty_rows.push(row.clone());
                    non_empty_rows.push(row.clone());
                    continue;
                }
            };

            for (pattern, row) in self.flatten_or(column.pattern, row) {
                let mut columns = row.columns;

                // We should only be able to reach list patterns here for well
                // typed code. Invalid patterns should have been caught by
                // earlier analysis.
                match self.pattern(pattern) {
                    Pattern::EmptyList => {
                        empty_rows.push(Row::new(columns, row.guard, row.body));
                    }

                    Pattern::List { first, rest } => {
                        columns.push(Column::new(first_var.clone(), *first));
                        columns.push(Column::new(rest_var.clone(), *rest));
                        non_empty_rows.push(Row::new(columns, row.guard, row.body));
                    }

                    pattern @ (Pattern::Or { .. }
                    | Pattern::Int { .. }
                    | Pattern::Float { .. }
                    | Pattern::Tuple { .. }
                    | Pattern::String { .. }
                    | Pattern::Discard
                    | Pattern::Assign { .. }
                    | Pattern::Variable { .. }
                    | Pattern::BitArray { .. }
                    | Pattern::Constructor { .. }
                    | Pattern::StringPrefix { .. }) => {
                        panic!("Unexpected non-list pattern {:?}", pattern)
                    }
                };
            }
        }

        Decision::List {
            variable: branch_var,
            empty: Box::new(self.compile_rows(empty_rows)),
            non_empty: Box::new(NonEmptyListDecision {
                first: first_var,
                rest: rest_var,
                decision: self.compile_rows(non_empty_rows),
            }),
        }
    }

    /// Moves variable-only patterns/tests into the right-hand side/body of a
    /// case.
    ///
    /// This turns cases like this:
    ///
    /// ```text
    /// case one -> print(one)
    /// case _ -> print("nothing")
    /// ```
    ///
    /// Into this:
    ///
    /// ```text
    /// case -> {
    ///     let one = it
    ///     print(one)
    /// }
    /// case -> {
    ///     print("nothing")
    /// }
    /// ```
    ///
    /// Where `it` is a variable holding the value `case one` is compared
    /// against, and the case/row has no patterns (i.e. always matches).
    fn move_unconditional_patterns(&self, row: Row) -> Row {
        let mut bindings = row.body.bindings;
        let mut columns = Vec::new();
        let mut iterator = row.columns.into_iter();
        let mut next = iterator.next();

        while let Some(column) = next {
            match self.pattern(column.pattern) {
                Pattern::Discard => {
                    next = iterator.next();
                }

                Pattern::Variable { name: bind } => {
                    next = iterator.next();
                    bindings.push((bind.clone(), column.variable));
                }

                Pattern::Assign { name, pattern } => {
                    next = Some(Column::new(column.variable.clone(), *pattern));
                    bindings.push((name.clone(), column.variable));
                }

                Pattern::Or { .. }
                | Pattern::Int { .. }
                | Pattern::List { .. }
                | Pattern::Float { .. }
                | Pattern::Tuple { .. }
                | Pattern::String { .. }
                | Pattern::EmptyList
                | Pattern::BitArray { .. }
                | Pattern::Constructor { .. }
                | Pattern::StringPrefix { .. } => {
                    next = iterator.next();
                    columns.push(column);
                }
            }
        }

        Row {
            columns,
            guard: row.guard,
            body: Body {
                bindings,
                clause_index: row.body.clause_index,
            },
        }
    }

    /// Given a row, returns the kind of branch that is referred to the
    /// most across all rows.
    ///
    /// # Panics
    ///
    /// Panics if there or no rows, or if the first row has no columns.
    ///
    fn branch_mode(&self, all_rows: &[Row]) -> BranchMode {
        let mut counts = HashMap::new();

        for row in all_rows {
            for col in &row.columns {
                *counts.entry(col.variable.id).or_insert(0_usize) += 1
            }
        }

        let variable = all_rows
            .first()
            .expect("Must have at least one row")
            .columns
            .iter()
            .map(|col| col.variable.clone())
            .max_by_key(|var| counts.get(&var.id).copied().unwrap_or(0))
            .expect("The first row must have at least one column");

        match collapse_links(variable.type_.clone()).as_ref() {
            Type::Fn { .. } | Type::Var { .. } => BranchMode::Infinite { variable },

            Type::Named { module, name, .. }
                if is_prelude_module(module)
                    && (name == "Int"
                        || name == "Float"
                        || name == "String"
                        || name == "BitArray") =>
            {
                BranchMode::Infinite { variable }
            }

            Type::Named {
                module, name, args, ..
            } if is_prelude_module(module) && name == "List" => BranchMode::List {
                variable,
                element_type: args.get(0).expect("Lists have 1 argument").clone(),
            },

            Type::Named { module, name, .. } => {
                let constructors = self
                    .custom_type_info(module, name)
                    .expect("Custom type variants must exist")
                    .to_vec();
                BranchMode::NamedType {
                    variable,
                    constructors,
                }
            }

            Type::Tuple { elems } => BranchMode::Tuple {
                variable,
                types: elems.clone(),
            },
        }
    }

    /// Returns a new variable to use in the decision tree.
    ///
    /// In a real compiler you'd have to ensure these variables don't conflict
    /// with other variables.
    pub fn new_variable(&mut self, type_: Arc<Type>) -> Variable {
        let var = Variable {
            id: self.variable_id,
            type_,
        };

        self.variable_id += 1;
        var
    }

    fn custom_type_info(
        &self,
        module: &EcoString,
        name: &EcoString,
    ) -> Result<&Vec<TypeValueConstructor>, UnknownTypeConstructorError> {
        self.environment.get_constructors_for_type(module, name)
    }

    fn constructor_parameter_variables(
        &mut self,
        type_: &Arc<Type>,
        parameters: &[TypeValueConstructorParameter],
    ) -> Vec<Variable> {
        parameters
            .iter()
            .map(|p| self.constructor_parameter_variable(type_, p))
            .collect_vec()
    }

    fn constructor_parameter_variable(
        &mut self,
        type_: &Arc<Type>,
        parameter: &TypeValueConstructorParameter,
    ) -> Variable {
        let type_ = match parameter.generic_type_parameter_index {
            None => parameter.type_.clone(),
            Some(i) => generic_named_type_parameter(type_, i).unwrap(),
        };
        self.new_variable(type_)
    }

    fn new_variables(&mut self, type_ids: &[Arc<Type>]) -> Vec<Variable> {
        type_ids
            .iter()
            .map(|t| self.new_variable(t.clone()))
            .collect()
    }
}

fn generic_named_type_parameter(t: &Type, i: usize) -> Option<Arc<Type>> {
    match t {
        Type::Named { args, .. } => args.get(i).cloned(),

        Type::Var { type_, .. } => match &*type_.borrow() {
            TypeVar::Link { type_ } => generic_named_type_parameter(&type_, i),
            TypeVar::Unbound { .. } | TypeVar::Generic { .. } => None,
        },

        Type::Fn { .. } | Type::Tuple { .. } => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum BranchMode {
    Infinite {
        variable: Variable,
    },
    Tuple {
        variable: Variable,
        types: Vec<Arc<Type>>,
    },
    List {
        variable: Variable,
        element_type: Arc<Type>,
    },
    NamedType {
        variable: Variable,
        constructors: Vec<TypeValueConstructor>,
    },
}
