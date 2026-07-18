//! The expansion phase sits between parsing and evaluation. Its only job is
//! to turn `for` into a `Sequence` of unrolled, `let`-wrapped iterations,
//! leaving everything else structurally unchanged. `for` must never reach
//! the evaluator; `loop`, `def`, `fn`, `if`, and `break` are left alone
//! here and are given meaning later, by the lowerer/evaluator.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::Expr;
use crate::core::CoreExpr;
use crate::error::ExpandError;

/// What the expander may consult to resolve a global name to a known
/// integer constant. Implemented by the runtime `GlobalStore` (paired with
/// a `GlobalRegistry`) for normal execution, and by a compile-only
/// constant environment when generating IR without running any code.
pub trait ForConstantSource {
    /// Returns the current known integer value of global `name`, if it is
    /// interned and currently resolvable as a constant integer.
    fn integer_constant(&self, name: &str) -> Option<i64>;
}

/// The maximum total number of loop iterations a single top-level
/// expression's `for` forms may unroll to (nested `for` counts multiply).
/// Guards against unbounded memory use from a runaway expansion.
pub const DEFAULT_MAX_FOR_EXPANSIONS: usize = 65_536;

#[derive(Debug, Clone, Copy)]
pub struct ExpansionLimits {
    pub max_for_expansions: usize,
}

impl Default for ExpansionLimits {
    fn default() -> Self {
        ExpansionLimits {
            max_for_expansions: DEFAULT_MAX_FOR_EXPANSIONS,
        }
    }
}

/// Expansion-time context: a source of known global integer constants
/// (for resolving integer `def`s as `for` constants) and the enclosing
/// `for` variables' current values (for nested `for` ranges).
pub struct ExpansionContext<'a> {
    constants: &'a dyn ForConstantSource,
    limits: ExpansionLimits,
    for_vars: HashMap<String, i64>,
    /// Remaining iteration budget, shared (via `Rc`) across the whole
    /// expansion of one top-level expression, so nested `for` bodies all
    /// draw from the same pool rather than each getting a fresh limit.
    budget: Rc<Cell<usize>>,
}

impl<'a> ExpansionContext<'a> {
    pub fn new(constants: &'a dyn ForConstantSource) -> Self {
        ExpansionContext::with_limits(constants, ExpansionLimits::default())
    }

    pub fn with_limits(constants: &'a dyn ForConstantSource, limits: ExpansionLimits) -> Self {
        ExpansionContext {
            constants,
            limits,
            for_vars: HashMap::new(),
            budget: Rc::new(Cell::new(limits.max_for_expansions)),
        }
    }

    fn child_with_for_var(&self, name: &str, value: i64) -> ExpansionContext<'_> {
        let mut for_vars = self.for_vars.clone();
        for_vars.insert(name.to_string(), value);
        ExpansionContext {
            constants: self.constants,
            limits: self.limits,
            for_vars,
            budget: Rc::clone(&self.budget),
        }
    }
}

/// Expands a single reader expression into a core expression. `for` forms
/// are unrolled here; every other form is copied structurally, with nested
/// subexpressions expanded recursively.
pub fn expand(expr: &Expr, context: &ExpansionContext) -> Result<CoreExpr, ExpandError> {
    match expr {
        Expr::Int(n) => Ok(CoreExpr::Int(*n)),
        Expr::Bool(b) => Ok(CoreExpr::Bool(*b)),
        Expr::String(s) => Ok(CoreExpr::String(s.clone())),
        Expr::Symbol(name) => Ok(CoreExpr::Symbol(name.clone())),
        Expr::List(items) => expand_list(items, context),
    }
}

fn expand_list(items: &[Expr], context: &ExpansionContext) -> Result<CoreExpr, ExpandError> {
    if let Some(Expr::Symbol(name)) = items.first()
        && name == "for"
    {
        return expand_for(&items[1..], context);
    }

    let mut expanded = Vec::with_capacity(items.len());
    for item in items {
        expanded.push(expand(item, context)?);
    }
    Ok(CoreExpr::List(expanded))
}

/// `(for (name start end [step]) body)`
fn expand_for(rest: &[Expr], context: &ExpansionContext) -> Result<CoreExpr, ExpandError> {
    let [binding_expr, body] = rest else {
        return Err(ExpandError::InvalidForSyntax);
    };

    let Expr::List(binding_items) = binding_expr else {
        return Err(ExpandError::InvalidForBinding);
    };

    let (name, start_expr, end_expr, step_expr) = match binding_items.as_slice() {
        [name, start, end] => (name, start, end, None),
        [name, start, end, step] => (name, start, end, Some(step)),
        _ => return Err(ExpandError::InvalidForBinding),
    };

    let Expr::Symbol(var_name) = name else {
        return Err(ExpandError::InvalidForBinding);
    };

    let start = eval_constant_int(start_expr, context)?;
    let end = eval_constant_int(end_expr, context)?;
    let step = match step_expr {
        Some(e) => eval_constant_int(e, context)?,
        None => 1,
    };
    if step == 0 {
        return Err(ExpandError::ZeroForStep);
    }

    let iterations = compute_iterations(start, end, step);

    let remaining = context.budget.get();
    if iterations > remaining {
        return Err(ExpandError::ForExpansionLimitExceeded);
    }
    context.budget.set(remaining - iterations);

    let mut sequence = Vec::with_capacity(iterations);
    let mut current = start;
    for _ in 0..iterations {
        let iter_context = context.child_with_for_var(var_name, current);
        let core_body = expand(body, &iter_context)?;
        sequence.push(CoreExpr::List(vec![
            CoreExpr::Symbol("let".to_string()),
            CoreExpr::List(vec![CoreExpr::List(vec![
                CoreExpr::Symbol(var_name.clone()),
                CoreExpr::Int(current),
            ])]),
            core_body,
        ]));
        // `iterations` was computed from the same start/end/step, so this
        // add cannot overflow before the loop ends.
        current += step;
    }

    Ok(CoreExpr::Sequence(sequence))
}

/// Computes the iteration count for a half-open `[start, end)` range with
/// the given step, matching `loop`'s runtime continuation rule (`current <
/// end` for a positive step, `current > end` for a negative one). Uses
/// `u64` arithmetic throughout so it cannot overflow even at the `i64`
/// extremes (e.g. `start = i64::MIN, end = i64::MAX`).
fn compute_iterations(start: i64, end: i64, step: i64) -> usize {
    if step > 0 {
        if start >= end {
            return 0;
        }
        let span = end.wrapping_sub(start) as u64;
        span.div_ceil(step as u64) as usize
    } else {
        if start <= end {
            return 0;
        }
        let span = start.wrapping_sub(end) as u64;
        // `step` is negative and not `i64::MIN` (its magnitude is checked
        // by the caller only for zero, but `-step` on `i64::MIN` would
        // overflow); widen through i128 to negate safely.
        let magnitude = (-(step as i128)) as u64;
        span.div_ceil(magnitude) as usize
    }
}

/// Evaluates an expression as an expansion-time integer constant. Only a
/// small, deliberately restricted sublanguage is allowed: integer
/// literals, enclosing `for` variables, current global integer `def`s, and
/// checked `+ - * / %` applications. Anything else (strings, `fn`, `print`,
/// `loop`, user function calls, runtime locals) is rejected so `for`'s
/// range can never depend on a runtime value.
fn eval_constant_int(expr: &Expr, context: &ExpansionContext) -> Result<i64, ExpandError> {
    match expr {
        Expr::Int(n) => Ok(*n),
        Expr::Bool(_) => Err(ExpandError::NonConstantForBound),
        Expr::String(_) => Err(ExpandError::NonIntegerForBound),
        Expr::Symbol(name) => {
            if let Some(&value) = context.for_vars.get(name) {
                return Ok(value);
            }
            context
                .constants
                .integer_constant(name)
                .ok_or(ExpandError::NonConstantForBound)
        }
        Expr::List(items) => eval_constant_application(items, context),
    }
}

fn eval_constant_application(
    items: &[Expr],
    context: &ExpansionContext,
) -> Result<i64, ExpandError> {
    let [Expr::Symbol(op), lhs, rhs] = items else {
        return Err(ExpandError::NonConstantForBound);
    };
    if !matches!(op.as_str(), "+" | "-" | "*" | "/" | "%") {
        return Err(ExpandError::NonConstantForBound);
    }

    let a = eval_constant_int(lhs, context)?;
    let b = eval_constant_int(rhs, context)?;

    match op.as_str() {
        "+" => a.checked_add(b).ok_or(ExpandError::ConstantIntegerOverflow),
        "-" => a.checked_sub(b).ok_or(ExpandError::ConstantIntegerOverflow),
        "*" => a.checked_mul(b).ok_or(ExpandError::ConstantIntegerOverflow),
        "/" => {
            if b == 0 {
                return Err(ExpandError::ConstantDivisionByZero);
            }
            a.checked_div(b).ok_or(ExpandError::ConstantIntegerOverflow)
        }
        "%" => {
            if b == 0 {
                return Err(ExpandError::ConstantRemainderByZero);
            }
            a.checked_rem(b).ok_or(ExpandError::ConstantIntegerOverflow)
        }
        _ => unreachable!("checked above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_one;
    use std::cell::RefCell;
    use std::collections::HashMap as Map;

    /// A simple in-memory constant source for tests: a plain name -> i64
    /// map, standing in for the runtime `GlobalStore`.
    #[derive(Default)]
    struct TestConstants(RefCell<Map<String, i64>>);

    impl TestConstants {
        fn with(pairs: &[(&str, i64)]) -> Self {
            let mut map = Map::new();
            for (name, value) in pairs {
                map.insert((*name).to_string(), *value);
            }
            TestConstants(RefCell::new(map))
        }
    }

    impl ForConstantSource for TestConstants {
        fn integer_constant(&self, name: &str) -> Option<i64> {
            self.0.borrow().get(name).copied()
        }
    }

    fn expand_source(source: &str) -> Result<CoreExpr, ExpandError> {
        let constants = TestConstants::default();
        let expr = parse_one(source).expect("should parse");
        let context = ExpansionContext::new(&constants);
        expand(&expr, &context)
    }

    #[test]
    fn non_for_expressions_pass_through_structurally() {
        let core = expand_source("(+ 1 2)").unwrap();
        assert_eq!(
            core,
            CoreExpr::List(vec![
                CoreExpr::Symbol("+".to_string()),
                CoreExpr::Int(1),
                CoreExpr::Int(2),
            ])
        );
    }

    #[test]
    fn for_expands_to_sequence_of_let_wrapped_iterations() {
        let core = expand_source("(for (i 0 3) i)").unwrap();
        let CoreExpr::Sequence(iterations) = core else {
            panic!("expected Sequence");
        };
        assert_eq!(iterations.len(), 3);
    }

    #[test]
    fn for_zero_iterations_is_empty_sequence() {
        let core = expand_source("(for (i 4 0) i)").unwrap();
        assert_eq!(core, CoreExpr::Sequence(vec![]));
    }

    #[test]
    fn for_negative_step() {
        let core = expand_source("(for (i 4 0 -1) i)").unwrap();
        let CoreExpr::Sequence(iterations) = core else {
            panic!("expected Sequence");
        };
        assert_eq!(iterations.len(), 4);
    }

    #[test]
    fn for_constant_expression_bound() {
        let core = expand_source("(for (i 0 (+ 2 2)) i)").unwrap();
        let CoreExpr::Sequence(iterations) = core else {
            panic!("expected Sequence");
        };
        assert_eq!(iterations.len(), 4);
    }

    #[test]
    fn for_zero_step_is_error() {
        let result = expand_source("(for (i 0 10 0) i)");
        assert!(matches!(result, Err(ExpandError::ZeroForStep)));
    }

    #[test]
    fn for_runtime_bound_is_error() {
        // Nested inside a `fn` whose parameter is a runtime value: `count`
        // is not resolvable as an expansion-time constant.
        let result = expand_source("(fn (count) (for (i 0 count) i))");
        assert!(matches!(result, Err(ExpandError::NonConstantForBound)));
    }

    #[test]
    fn for_division_by_zero_constant_is_error() {
        let result = expand_source("(for (i 0 (/ 10 0)) i)");
        assert!(matches!(result, Err(ExpandError::ConstantDivisionByZero)));
    }

    #[test]
    fn for_expansion_limit_exceeded() {
        let constants = TestConstants::default();
        let expr = parse_one("(for (i 0 10) i)").unwrap();
        let context = ExpansionContext::with_limits(
            &constants,
            ExpansionLimits {
                max_for_expansions: 5,
            },
        );
        let result = expand(&expr, &context);
        assert!(matches!(
            result,
            Err(ExpandError::ForExpansionLimitExceeded)
        ));
    }

    #[test]
    fn nested_for_uses_outer_variable() {
        // (for (x 0 3) (for (y 0 (+ x 1)) body)) unrolls to 1+2+3 = 6 inner
        // iterations.
        let constants = TestConstants::default();
        let expr = parse_one("(for (x 0 3) (for (y 0 (+ x 1)) y))").unwrap();
        let context = ExpansionContext::new(&constants);
        let core = expand(&expr, &context).unwrap();
        let CoreExpr::Sequence(outer) = core else {
            panic!("expected Sequence");
        };
        assert_eq!(outer.len(), 3);
    }

    #[test]
    fn global_integer_constant_bound() {
        let constants = TestConstants::with(&[("count", 3)]);
        let expr = parse_one("(for (i 0 count) i)").unwrap();
        let context = ExpansionContext::new(&constants);
        let core = expand(&expr, &context).unwrap();
        let CoreExpr::Sequence(iterations) = core else {
            panic!("expected Sequence");
        };
        assert_eq!(iterations.len(), 3);
    }
}
