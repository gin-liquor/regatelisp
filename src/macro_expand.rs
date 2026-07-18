use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use crate::ast::{Expr, ExprKind};
use crate::datum::{datum_to_expr, expr_to_datum};
use crate::error::{LispError, MacroExpandError};
use crate::interpreter::Interpreter;
use crate::property::Properties;
use crate::value::Value;

const MAX_EXPANSION_DEPTH: usize = 256;
const MAX_MACRO_INVOCATIONS: usize = 10_000;
const RESERVED_MACRO_NAMES: &[&str] = &[
    "fn",
    "let",
    "for",
    "loop",
    "def",
    "defmacro",
    "if",
    "break",
    "quote",
    "quasiquote",
    "unquote",
    "gensym",
    "meta",
    "true",
    "false",
];

#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Expr,
    pub properties: Properties,
}

#[derive(Debug, Clone, Default)]
pub struct MacroEnv {
    definitions: BTreeMap<String, MacroDef>,
}

impl MacroEnv {
    fn get(&self, name: &str) -> Option<&MacroDef> {
        self.definitions.get(name)
    }
}

pub struct MacroExpansionSession {
    env: MacroEnv,
    next_gensym_id: u64,
    invocations: usize,
    stack: Vec<MacroExpansionFrame>,
    limits: MacroExpansionLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroExpansionFrame {
    pub macro_name: String,
    pub invocation_span: Option<crate::error::Span>,
    pub definition_span: Option<crate::error::Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroExpansionLimits {
    pub max_depth: usize,
    pub max_invocations: usize,
}

impl Default for MacroExpansionLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_EXPANSION_DEPTH,
            max_invocations: MAX_MACRO_INVOCATIONS,
        }
    }
}

impl MacroExpansionSession {
    pub fn new(next_gensym_id: u64) -> Self {
        Self::with_limits(next_gensym_id, MacroExpansionLimits::default())
    }

    pub fn with_limits(next_gensym_id: u64, limits: MacroExpansionLimits) -> Self {
        Self {
            env: MacroEnv::default(),
            next_gensym_id,
            invocations: 0,
            stack: Vec::new(),
            limits,
        }
    }

    pub fn expand_program(&mut self, expressions: &[Expr]) -> Result<Vec<Expr>, MacroExpandError> {
        let mut ordinary = Vec::with_capacity(expressions.len());
        for expression in expressions {
            if let Some(definition) = parse_macro_definition(expression)? {
                if self
                    .env
                    .definitions
                    .insert(definition.name.clone(), definition.clone())
                    .is_some()
                {
                    return Err(MacroExpandError::DuplicateMacroDefinition(definition.name));
                }
            } else {
                reject_nested_macro_definition(expression, 0)?;
                ordinary.push(expression.clone());
            }
        }

        ordinary
            .iter()
            .map(|expression| self.expand_expr(expression, 0))
            .collect()
    }

    pub fn next_gensym_id(&self) -> u64 {
        self.next_gensym_id
    }

    fn expand_expr(&mut self, expression: &Expr, depth: usize) -> Result<Expr, MacroExpandError> {
        let ExprKind::List(items) = expression.kind() else {
            return Ok(expression.clone());
        };
        let Some(first) = items.first() else {
            return Ok(expression.clone());
        };

        if let ExprKind::Symbol(name) = first.kind() {
            match name.as_str() {
                "quote" => return Ok(expression.clone()),
                "quasiquote" => {
                    let mut result = expression.clone();
                    if let ExprKind::List(result_items) = result.kind_mut()
                        && let Some(template) = result_items.get_mut(1)
                    {
                        *template = self.expand_quasiquote(template, 1, depth)?;
                    }
                    return Ok(result);
                }
                "meta" => {
                    let mut result = expression.clone();
                    if let ExprKind::List(result_items) = result.kind_mut()
                        && let Some(value) = result_items.get_mut(2)
                    {
                        *value = self.expand_expr(value, depth)?;
                    }
                    return Ok(result);
                }
                "defmacro" => return Err(MacroExpandError::GeneratedMacroDefinition),
                _ => {}
            }

            if let Some(definition) = self.env.get(name).cloned() {
                return self.invoke(&definition, expression, &items[1..], depth);
            }
        }

        let mut result = expression.clone();
        if let ExprKind::List(result_items) = result.kind_mut() {
            for item in result_items {
                *item = self.expand_expr(item, depth)?;
            }
        }
        Ok(result)
    }

    fn expand_quasiquote(
        &mut self,
        expression: &Expr,
        quasiquote_depth: usize,
        macro_depth: usize,
    ) -> Result<Expr, MacroExpandError> {
        let ExprKind::List(items) = expression.kind() else {
            return Ok(expression.clone());
        };
        if let Some(ExprKind::Symbol(name)) = items.first().map(Expr::kind) {
            if name == "quote" {
                return Ok(expression.clone());
            }
            if name == "unquote" && quasiquote_depth == 1 {
                let mut result = expression.clone();
                if let ExprKind::List(result_items) = result.kind_mut()
                    && let Some(value) = result_items.get_mut(1)
                {
                    *value = self.expand_expr(value, macro_depth)?;
                }
                return Ok(result);
            }
        }

        let mut result = expression.clone();
        if let ExprKind::List(result_items) = result.kind_mut() {
            let nested_depth = match items.first().map(Expr::kind) {
                Some(ExprKind::Symbol(name)) if name == "quasiquote" => quasiquote_depth + 1,
                Some(ExprKind::Symbol(name)) if name == "unquote" => quasiquote_depth - 1,
                _ => quasiquote_depth,
            };
            for item in result_items {
                *item = self.expand_quasiquote(item, nested_depth, macro_depth)?;
            }
        }
        Ok(result)
    }

    fn invoke(
        &mut self,
        definition: &MacroDef,
        invocation: &Expr,
        arguments: &[Expr],
        depth: usize,
    ) -> Result<Expr, MacroExpandError> {
        if arguments.len() != definition.params.len() {
            return Err(MacroExpandError::MacroArityMismatch {
                name: definition.name.clone(),
                expected: definition.params.len(),
                got: arguments.len(),
            });
        }
        if depth >= self.limits.max_depth {
            return Err(MacroExpandError::MacroExpansionDepthExceeded {
                stack: self.stack_names(),
            });
        }
        if self.invocations >= self.limits.max_invocations {
            return Err(MacroExpandError::MacroExpansionBudgetExceeded {
                stack: self.stack_names(),
            });
        }
        self.invocations += 1;
        self.stack.push(MacroExpansionFrame {
            macro_name: definition.name.clone(),
            invocation_span: None,
            definition_span: None,
        });

        let mut evaluator = Interpreter::new_compile_time(Vec::new());
        evaluator.set_next_gensym_id(self.next_gensym_id);
        for (parameter, argument) in definition.params.iter().zip(arguments) {
            evaluator.define_compile_time_binding(
                parameter,
                Value::Datum(Rc::new(expr_to_datum(argument))),
            );
        }

        let prepared_body = match self.expand_expr(&definition.body, depth + 1) {
            Ok(body) => body,
            Err(error) => {
                self.stack.pop();
                return Err(error);
            }
        };
        let evaluated = evaluator
            .eval_expr_unexpanded(&prepared_body)
            .map_err(|error| MacroExpandError::MacroEvaluationFailed {
                name: definition.name.clone(),
                cause: error.to_string(),
                stack: self.stack_names(),
            });
        self.next_gensym_id = evaluator.next_gensym_id();
        let value = match evaluated {
            Ok(value) => value,
            Err(error) => {
                self.stack.pop();
                return Err(error);
            }
        };
        let Value::Datum(datum) = value else {
            let error = MacroExpandError::MacroResultIsNotDatum {
                name: definition.name.clone(),
                actual: value.type_name().to_string(),
                stack: self.stack_names(),
            };
            self.stack.pop();
            return Err(error);
        };

        let mut result = datum_to_expr(&datum);
        restore_argument_properties(&mut result, arguments);
        restore_template_properties(&mut result, &definition.body, &definition.params, arguments);
        result
            .properties_mut()
            .merge_missing_from(invocation.properties());
        let expanded = self.expand_expr(&result, depth + 1);
        self.stack.pop();
        expanded
    }

    fn stack_names(&self) -> Vec<String> {
        self.stack
            .iter()
            .map(|frame| frame.macro_name.clone())
            .collect()
    }
}

pub fn expand_program(
    expressions: &[Expr],
    next_gensym_id: u64,
) -> Result<(Vec<Expr>, u64), LispError> {
    let mut session = MacroExpansionSession::new(next_gensym_id);
    let expanded = session.expand_program(expressions)?;
    Ok((expanded, session.next_gensym_id()))
}

fn parse_macro_definition(expression: &Expr) -> Result<Option<MacroDef>, MacroExpandError> {
    let ExprKind::List(items) = expression.kind() else {
        return Ok(None);
    };
    let Some(ExprKind::Symbol(keyword)) = items.first().map(Expr::kind) else {
        return Ok(None);
    };
    if keyword == "meta" {
        let [_, _, inner] = items.as_slice() else {
            return Ok(None);
        };
        return parse_macro_definition(inner);
    }
    if keyword != "defmacro" {
        return Ok(None);
    }
    let [_, name_expr, params_expr, body] = items.as_slice() else {
        return Err(MacroExpandError::InvalidMacroDefinition);
    };
    let ExprKind::Symbol(name) = name_expr.kind() else {
        return Err(MacroExpandError::InvalidMacroName);
    };
    if RESERVED_MACRO_NAMES.contains(&name.as_str()) {
        return Err(MacroExpandError::ReservedMacroName(name.clone()));
    }
    let ExprKind::List(param_exprs) = params_expr.kind() else {
        return Err(MacroExpandError::InvalidMacroParameterList);
    };
    let mut params = Vec::with_capacity(param_exprs.len());
    let mut seen = HashSet::new();
    for parameter in param_exprs {
        let ExprKind::Symbol(parameter) = parameter.kind() else {
            return Err(MacroExpandError::InvalidMacroParameterList);
        };
        if !seen.insert(parameter.clone()) {
            return Err(MacroExpandError::DuplicateMacroParameter(parameter.clone()));
        }
        params.push(parameter.clone());
    }
    reject_forbidden_macro_operations(body)?;
    Ok(Some(MacroDef {
        name: name.clone(),
        params,
        body: body.clone(),
        properties: expression.properties().clone(),
    }))
}

fn reject_forbidden_macro_operations(expression: &Expr) -> Result<(), MacroExpandError> {
    reject_forbidden_at_depth(expression, 0)
}

fn reject_forbidden_at_depth(
    expression: &Expr,
    quasiquote_depth: usize,
) -> Result<(), MacroExpandError> {
    let ExprKind::List(items) = expression.kind() else {
        return Ok(());
    };
    if let Some(ExprKind::Symbol(name)) = items.first().map(Expr::kind) {
        if name == "quote" {
            return Ok(());
        }
        if quasiquote_depth == 0 && matches!(name.as_str(), "print" | "def" | "defmacro") {
            return Err(MacroExpandError::ForbiddenMacroOperation(name.clone()));
        }
        if name == "quasiquote" {
            for item in items.iter().skip(1) {
                reject_forbidden_at_depth(item, quasiquote_depth + 1)?;
            }
            return Ok(());
        }
        if name == "unquote" && quasiquote_depth > 0 {
            for item in items.iter().skip(1) {
                reject_forbidden_at_depth(item, quasiquote_depth - 1)?;
            }
            return Ok(());
        }
    }
    for item in items {
        reject_forbidden_at_depth(item, quasiquote_depth)?;
    }
    Ok(())
}

fn reject_nested_macro_definition(
    expression: &Expr,
    quasiquote_depth: usize,
) -> Result<(), MacroExpandError> {
    let ExprKind::List(items) = expression.kind() else {
        return Ok(());
    };
    if let Some(ExprKind::Symbol(name)) = items.first().map(Expr::kind) {
        if name == "quote" {
            return Ok(());
        }
        if name == "defmacro" && quasiquote_depth == 0 {
            return Err(MacroExpandError::NestedMacroDefinition);
        }
        if name == "meta" {
            if let Some(value) = items.get(2) {
                reject_nested_macro_definition(value, quasiquote_depth)?;
            }
            return Ok(());
        }
        if name == "quasiquote" {
            for item in items.iter().skip(1) {
                reject_nested_macro_definition(item, quasiquote_depth + 1)?;
            }
            return Ok(());
        }
        if name == "unquote" && quasiquote_depth > 0 {
            for item in items.iter().skip(1) {
                reject_nested_macro_definition(item, quasiquote_depth - 1)?;
            }
            return Ok(());
        }
    }
    for item in items {
        reject_nested_macro_definition(item, quasiquote_depth)?;
    }
    Ok(())
}

fn restore_argument_properties(result: &mut Expr, arguments: &[Expr]) {
    let result_datum = expr_to_datum(result);
    if let Some(argument) = arguments
        .iter()
        .find(|argument| expr_to_datum(argument) == result_datum)
    {
        *result = argument.clone();
        return;
    }
    if let ExprKind::List(items) = result.kind_mut() {
        for item in items {
            restore_argument_properties(item, arguments);
        }
    }
}

fn restore_template_properties(
    result: &mut Expr,
    body: &Expr,
    params: &[String],
    arguments: &[Expr],
) {
    let ExprKind::List(body_items) = body.kind() else {
        return;
    };
    if !matches!(body_items.first().map(Expr::kind), Some(ExprKind::Symbol(name)) if name == "quasiquote")
    {
        return;
    }
    if let Some(template) = body_items.get(1) {
        restore_template_node(result, template, params, arguments, 1);
    }
}

fn restore_template_node(
    result: &mut Expr,
    template: &Expr,
    params: &[String],
    arguments: &[Expr],
    depth: usize,
) {
    if let ExprKind::List(template_items) = template.kind()
        && matches!(template_items.first().map(Expr::kind), Some(ExprKind::Symbol(name)) if name == "unquote")
        && depth == 1
    {
        if let Some(ExprKind::Symbol(parameter)) = template_items.get(1).map(Expr::kind)
            && let Some(index) = params.iter().position(|name| name == parameter)
        {
            copy_properties_recursively(result, &arguments[index]);
        }
        return;
    }

    result
        .properties_mut()
        .merge_missing_from(template.properties());
    let (ExprKind::List(result_items), ExprKind::List(template_items)) =
        (result.kind_mut(), template.kind())
    else {
        return;
    };
    let nested_depth = match template_items.first().map(Expr::kind) {
        Some(ExprKind::Symbol(name)) if name == "quasiquote" => depth + 1,
        Some(ExprKind::Symbol(name)) if name == "unquote" && depth > 1 => depth - 1,
        _ => depth,
    };
    for (result_item, template_item) in result_items.iter_mut().zip(template_items) {
        restore_template_node(result_item, template_item, params, arguments, nested_depth);
    }
}

fn copy_properties_recursively(target: &mut Expr, source: &Expr) {
    target
        .properties_mut()
        .merge_missing_from(source.properties());
    if let (ExprKind::List(target_items), ExprKind::List(source_items)) =
        (target.kind_mut(), source.kind())
    {
        for (target_item, source_item) in target_items.iter_mut().zip(source_items) {
            copy_properties_recursively(target_item, source_item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn collects_forward_definition_and_expands_call() {
        let source = "(inc 4) (defmacro inc (x) (quasiquote (+ (unquote x) 1)))";
        let expressions = parser::parse_program(source).unwrap();
        let (expanded, _) = expand_program(&expressions, 0).unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].to_string(), "(+ 4 1)");
    }

    #[test]
    fn quote_shields_macro_calls() {
        let source = "(defmacro one () (quote 1)) (quote (one))";
        let expressions = parser::parse_program(source).unwrap();
        let (expanded, _) = expand_program(&expressions, 0).unwrap();
        assert_eq!(expanded[0].to_string(), "(quote (one))");
    }

    #[test]
    fn rejects_non_datum_result() {
        let source = "(defmacro bad () 1) (bad)";
        let expressions = parser::parse_program(source).unwrap();
        let error = expand_program(&expressions, 0).unwrap_err();
        assert!(matches!(
            error,
            LispError::Macro(MacroExpandError::MacroResultIsNotDatum { .. })
        ));
    }

    #[test]
    fn recursive_expansion_stops_at_the_depth_limit() {
        let expressions =
            parser::parse_program("(defmacro forever () (quasiquote (forever))) (forever)")
                .unwrap();
        let mut session = MacroExpansionSession::with_limits(
            0,
            MacroExpansionLimits {
                max_depth: 4,
                max_invocations: 100,
            },
        );
        let error = session.expand_program(&expressions).unwrap_err();
        assert!(matches!(
            error,
            MacroExpandError::MacroExpansionDepthExceeded { stack } if stack.len() == 4
        ));
    }

    #[test]
    fn separate_calls_share_the_invocation_budget() {
        let expressions =
            parser::parse_program("(defmacro one () (quote 1)) (one) (one) (one)").unwrap();
        let mut session = MacroExpansionSession::with_limits(
            0,
            MacroExpansionLimits {
                max_depth: 100,
                max_invocations: 2,
            },
        );
        assert!(matches!(
            session.expand_program(&expressions),
            Err(MacroExpandError::MacroExpansionBudgetExceeded { .. })
        ));
    }
}
