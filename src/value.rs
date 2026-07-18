use std::fmt;
use std::rc::Rc;

use crate::ids::FunctionId;

/// A built-in function. Builtins are ordinary global values, so they
/// participate in the same call path as closures and can be shadowed (via
/// `def`) like any other global. `Print` needs access to the
/// interpreter's output target, which a plain function pointer cannot
/// receive, so builtins are identified by tag and dispatched in
/// `builtin.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Print,
}

impl Builtin {
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Add => "+",
            Builtin::Sub => "-",
            Builtin::Mul => "*",
            Builtin::Div => "/",
            Builtin::Rem => "%",
            Builtin::Eq => "=",
            Builtin::Ne => "!=",
            Builtin::Lt => "<",
            Builtin::Le => "<=",
            Builtin::Gt => ">",
            Builtin::Ge => ">=",
            Builtin::Print => "print",
        }
    }

    /// Fixed argument count, or `None` for `print`, which takes a format
    /// string followed by a variable number of arguments.
    pub fn arity(self) -> Option<usize> {
        match self {
            Builtin::Add
            | Builtin::Sub
            | Builtin::Mul
            | Builtin::Div
            | Builtin::Rem
            | Builtin::Eq
            | Builtin::Ne
            | Builtin::Lt
            | Builtin::Le
            | Builtin::Gt
            | Builtin::Ge => Some(2),
            Builtin::Print => None,
        }
    }
}

/// A runtime closure: which function to call, plus the values it captured
/// explicitly at `MakeClosure` time. Unlike an environment-chain closure,
/// this holds no reference to any enclosing scope -- only the captured
/// values themselves, so a function's local `Frame` can be dropped as soon
/// as it returns without invalidating closures it created.
#[derive(Debug, Clone, PartialEq)]
pub struct Closure {
    pub function: FunctionId,
    pub captures: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    String(Rc<String>),
    Unit,
    Builtin(Builtin),
    Closure(Rc<Closure>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "integer",
            Value::Bool(_) => "boolean",
            Value::String(_) => "string",
            Value::Unit => "unit",
            Value::Builtin(_) => "builtin",
            Value::Closure(_) => "function",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Unit => write!(f, "()"),
            Value::Builtin(b) => write!(f, "<builtin:{}>", b.name()),
            Value::Closure(_) => write!(f, "<fn>"),
        }
    }
}
