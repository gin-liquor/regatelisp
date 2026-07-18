//! Implementations of the built-in functions stored in the initial
//! environment: arithmetic, `%`, and `print`. Only `print` needs access to
//! the interpreter's output target; the others ignore it.

use std::io::Write;

use crate::error::EvalError;
use crate::format::{parse_format_string, render};
use crate::value::{Builtin, Value};

pub fn apply_builtin(
    builtin: Builtin,
    args: &[Value],
    output: &mut dyn Write,
) -> Result<Value, EvalError> {
    if let Some(expected) = builtin.arity()
        && args.len() != expected
    {
        return Err(EvalError::WrongArgCount {
            expected,
            got: args.len(),
        });
    }

    match builtin {
        Builtin::Add => arithmetic(args, i64::checked_add),
        Builtin::Sub => arithmetic(args, i64::checked_sub),
        Builtin::Mul => arithmetic(args, i64::checked_mul),
        Builtin::Div => division(args),
        Builtin::Rem => remainder(args),
        Builtin::Eq | Builtin::Ne => equality(builtin, args),
        Builtin::Lt | Builtin::Le | Builtin::Gt | Builtin::Ge => ordering(builtin, args),
        Builtin::Print => print(args, output),
    }
}

fn equality(op: Builtin, args: &[Value]) -> Result<Value, EvalError> {
    let equal = match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Unit, Value::Unit) => true,
        (a, b) => {
            return Err(EvalError::ComparisonTypeMismatch {
                left: a.type_name(),
                right: b.type_name(),
            });
        }
    };
    Ok(Value::Bool(if op == Builtin::Eq { equal } else { !equal }))
}

fn ordering(op: Builtin, args: &[Value]) -> Result<Value, EvalError> {
    let (Value::Int(a), Value::Int(b)) = (&args[0], &args[1]) else {
        return Err(EvalError::UnorderableType(args[0].type_name()));
    };
    Ok(Value::Bool(match op {
        Builtin::Lt => a < b,
        Builtin::Le => a <= b,
        Builtin::Gt => a > b,
        Builtin::Ge => a >= b,
        _ => unreachable!(),
    }))
}

fn as_int(value: &Value) -> Result<i64, EvalError> {
    match value {
        Value::Int(n) => Ok(*n),
        _ => Err(EvalError::NonIntegerArgument),
    }
}

fn arithmetic(args: &[Value], op: fn(i64, i64) -> Option<i64>) -> Result<Value, EvalError> {
    let a = as_int(&args[0])?;
    let b = as_int(&args[1])?;
    op(a, b).map(Value::Int).ok_or(EvalError::IntegerOverflow)
}

fn division(args: &[Value]) -> Result<Value, EvalError> {
    let a = as_int(&args[0])?;
    let b = as_int(&args[1])?;
    if b == 0 {
        return Err(EvalError::DivisionByZero);
    }
    a.checked_div(b)
        .map(Value::Int)
        .ok_or(EvalError::IntegerOverflow)
}

fn remainder(args: &[Value]) -> Result<Value, EvalError> {
    let a = as_int(&args[0])?;
    let b = as_int(&args[1])?;
    if b == 0 {
        return Err(EvalError::DivisionByZero);
    }
    a.checked_rem(b)
        .map(Value::Int)
        .ok_or(EvalError::IntegerOverflow)
}

/// `(print format-string argument...)`. Builds the entire output in memory
/// first, so a formatting error never causes a partial write.
fn print(args: &[Value], output: &mut dyn Write) -> Result<Value, EvalError> {
    let Some(format_arg) = args.first() else {
        return Err(EvalError::WrongArgCount {
            expected: 1,
            got: 0,
        });
    };
    let Value::String(format_string) = format_arg else {
        return Err(EvalError::NonStringFormatArgument);
    };

    let parts = parse_format_string(format_string).map_err(EvalError::Format)?;
    let rendered = render(&parts, &args[1..]).map_err(EvalError::Format)?;

    output
        .write_all(rendered.as_bytes())
        .map_err(|e| EvalError::Io {
            message: e.to_string(),
        })?;

    Ok(Value::Unit)
}

/// Builds the initial environment bindings for all builtins.
pub fn all() -> Vec<(&'static str, Value)> {
    [
        Builtin::Add,
        Builtin::Sub,
        Builtin::Mul,
        Builtin::Div,
        Builtin::Rem,
        Builtin::Eq,
        Builtin::Ne,
        Builtin::Lt,
        Builtin::Le,
        Builtin::Gt,
        Builtin::Ge,
        Builtin::Print,
    ]
    .into_iter()
    .map(|b| (b.name(), Value::Builtin(b)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn out() -> Vec<u8> {
        Vec::new()
    }

    #[test]
    fn add_two_ints() {
        let mut o = out();
        let result = apply_builtin(Builtin::Add, &[Value::Int(1), Value::Int(2)], &mut o).unwrap();
        assert!(matches!(result, Value::Int(3)));
    }

    #[test]
    fn remainder_signs_follow_dividend() {
        let mut o = out();
        let cases = [(7, 3, 1), (-7, 3, -1), (7, -3, 1), (-7, -3, -1)];
        for (a, b, expected) in cases {
            let result =
                apply_builtin(Builtin::Rem, &[Value::Int(a), Value::Int(b)], &mut o).unwrap();
            assert!(matches!(result, Value::Int(n) if n == expected));
        }
    }

    #[test]
    fn remainder_by_zero_is_error() {
        let mut o = out();
        let result = apply_builtin(Builtin::Rem, &[Value::Int(10), Value::Int(0)], &mut o);
        assert!(matches!(result, Err(EvalError::DivisionByZero)));
    }

    #[test]
    fn remainder_overflow_is_error() {
        let mut o = out();
        let result = apply_builtin(
            Builtin::Rem,
            &[Value::Int(i64::MIN), Value::Int(-1)],
            &mut o,
        );
        assert!(matches!(result, Err(EvalError::IntegerOverflow)));
    }

    #[test]
    fn print_writes_to_output_and_returns_unit() {
        let mut o = out();
        let result = apply_builtin(
            Builtin::Print,
            &[Value::String(Rc::new("hello".to_string()))],
            &mut o,
        )
        .unwrap();
        assert!(matches!(result, Value::Unit));
        assert_eq!(o, b"hello");
    }

    #[test]
    fn print_first_argument_must_be_string() {
        let mut o = out();
        let result = apply_builtin(Builtin::Print, &[Value::Int(123)], &mut o);
        assert!(matches!(result, Err(EvalError::NonStringFormatArgument)));
    }

    #[test]
    fn print_error_leaves_output_untouched() {
        let mut o = out();
        let result = apply_builtin(
            Builtin::Print,
            &[Value::String(Rc::new("{} {}".to_string())), Value::Int(10)],
            &mut o,
        );
        assert!(result.is_err());
        assert!(o.is_empty());
    }
}
