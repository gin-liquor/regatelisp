use crate::ids::{CaptureSlot, FunctionId, GlobalId, LocalSlot, LoopId};
use crate::value::Value;
use std::fmt;

/// A byte offset into the source text, used to locate errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnrecognizedCharacter { ch: char, span: Span },
    InvalidInteger { text: String, span: Span },
    IntegerOutOfRange { text: String, span: Span },
    MalformedRadixLiteral { text: String, span: Span },
    UnterminatedString { span: Span },
    InvalidEscape { ch: char, span: Span },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnrecognizedCharacter { ch, span } => {
                write!(f, "unrecognized character '{ch}' at byte {}", span.start)
            }
            LexError::InvalidInteger { text, span } => {
                write!(f, "invalid integer '{text}' at byte {}", span.start)
            }
            LexError::IntegerOutOfRange { text, span } => {
                write!(
                    f,
                    "integer '{text}' out of i64 range at byte {}",
                    span.start
                )
            }
            LexError::MalformedRadixLiteral { text, span } => {
                write!(
                    f,
                    "malformed radix integer literal '{text}' at byte {}",
                    span.start
                )
            }
            LexError::UnterminatedString { span } => {
                write!(
                    f,
                    "unterminated string literal starting at byte {}",
                    span.start
                )
            }
            LexError::InvalidEscape { ch, span } => {
                write!(f, "invalid string escape '\\{ch}' at byte {}", span.start)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnexpectedEof,
    UnmatchedOpenParen { span: Span },
    UnmatchedCloseParen { span: Span },
    EmptyInput,
    ExtraTokensAfterExpression { span: Span },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedEof => write!(f, "unexpected end of input"),
            ParseError::UnmatchedOpenParen { span } => {
                write!(f, "unmatched opening parenthesis at byte {}", span.start)
            }
            ParseError::UnmatchedCloseParen { span } => {
                write!(f, "unmatched closing parenthesis at byte {}", span.start)
            }
            ParseError::EmptyInput => write!(f, "input is empty"),
            ParseError::ExtraTokensAfterExpression { span } => write!(
                f,
                "extra top-level expression starting at byte {}",
                span.start
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EvalError {
    Break(Value),
    InvalidBreakSyntax,
    UndefinedSymbol(String),
    EmptyApplication,
    NotCallable(String),
    WrongArgCount {
        expected: usize,
        got: usize,
    },
    NonIntegerArgument,
    InvalidIfSyntax,
    InvalidGeneralLoopSyntax,
    InvalidLoopStateBinding,
    DuplicateLoopState(String),
    InvalidLoopStateUpdate,
    NonBooleanCondition(&'static str),
    ComparisonTypeMismatch {
        left: &'static str,
        right: &'static str,
    },
    UnorderableType(&'static str),
    DivisionByZero,
    IntegerOverflow,
    MalformedFn,
    MalformedFnParams,
    DuplicateParameter(String),
    MalformedLet,
    MalformedLetBinding,
    DuplicateBinding(String),
    NonStringFormatArgument,
    Format(FormatError),
    Io {
        message: String,
    },
    InvalidLoopSyntax,
    InvalidLoopBinding,
    NonIntegerLoopBound,
    ZeroLoopStep,
    LoopCounterOverflow,
    InvalidDefSyntax,
    InvalidDefName,
    ReservedDefName(String),
    DefOutsideTopLevel,
    CannotConvertToDatum(&'static str),
    InvalidGensymPrefix(&'static str),
    GensymIdOverflow,
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Break(_) => write!(f, "break"),
            EvalError::InvalidBreakSyntax => write!(f, "malformed break expression"),
            EvalError::UndefinedSymbol(name) => write!(f, "undefined symbol: {name}"),
            EvalError::EmptyApplication => write!(f, "cannot evaluate empty list"),
            EvalError::NotCallable(repr) => write!(f, "value is not callable: {repr}"),
            EvalError::WrongArgCount { expected, got } => {
                write!(
                    f,
                    "wrong number of arguments: expected {expected}, got {got}"
                )
            }
            EvalError::NonIntegerArgument => write!(f, "arithmetic requires integer arguments"),
            EvalError::InvalidIfSyntax => write!(f, "malformed if expression"),
            EvalError::InvalidGeneralLoopSyntax => write!(f, "malformed general loop expression"),
            EvalError::InvalidLoopStateBinding => write!(f, "malformed loop state binding"),
            EvalError::DuplicateLoopState(n) => write!(f, "duplicate loop state: {n}"),
            EvalError::InvalidLoopStateUpdate => write!(f, "invalid loop state update"),
            EvalError::NonBooleanCondition(t) => write!(f, "if condition must be boolean, got {t}"),
            EvalError::ComparisonTypeMismatch { left, right } => {
                write!(f, "cannot compare {left} with {right}")
            }
            EvalError::UnorderableType(t) => write!(f, "ordering requires integers, got {t}"),
            EvalError::DivisionByZero => write!(f, "division by zero"),
            EvalError::IntegerOverflow => write!(f, "integer overflow"),
            EvalError::MalformedFn => write!(f, "malformed fn expression"),
            EvalError::MalformedFnParams => write!(f, "malformed fn parameter list"),
            EvalError::DuplicateParameter(name) => write!(f, "duplicate parameter name: {name}"),
            EvalError::MalformedLet => write!(f, "malformed let expression"),
            EvalError::MalformedLetBinding => write!(f, "malformed let binding"),
            EvalError::DuplicateBinding(name) => write!(f, "duplicate let binding name: {name}"),
            EvalError::NonStringFormatArgument => {
                write!(f, "print requires a string as its first argument")
            }
            EvalError::Format(e) => write!(f, "format error: {e}"),
            EvalError::Io { message } => write!(f, "I/O error: {message}"),
            EvalError::InvalidLoopSyntax => write!(f, "malformed loop expression"),
            EvalError::InvalidLoopBinding => write!(f, "malformed loop binding"),
            EvalError::NonIntegerLoopBound => write!(f, "loop bounds must be integers"),
            EvalError::ZeroLoopStep => write!(f, "loop step must not be zero"),
            EvalError::LoopCounterOverflow => write!(f, "loop counter overflow"),
            EvalError::InvalidDefSyntax => write!(f, "malformed def expression"),
            EvalError::InvalidDefName => write!(f, "def name must be a symbol"),
            EvalError::ReservedDefName(name) => {
                write!(
                    f,
                    "'{name}' is a reserved special-form name and cannot be defined"
                )
            }
            EvalError::DefOutsideTopLevel => write!(f, "def is only allowed at top level"),
            EvalError::CannotConvertToDatum(actual) => {
                write!(f, "unquote result cannot be converted to datum: {actual}")
            }
            EvalError::InvalidGensymPrefix(actual) => {
                write!(
                    f,
                    "gensym prefix must be an interned symbol datum, got {actual}"
                )
            }
            EvalError::GensymIdOverflow => write!(f, "gensym ID overflow"),
        }
    }
}

/// Errors produced while parsing or applying a `print` format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    UnmatchedOpenBrace,
    UnmatchedCloseBrace,
    MissingArgument {
        index: usize,
    },
    UnusedArgument {
        index: usize,
    },
    UnsupportedFormatType {
        type_char: char,
    },
    TypeMismatch {
        format_type: &'static str,
        value_type: &'static str,
    },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::UnmatchedOpenBrace => write!(f, "unmatched '{{' in format string"),
            FormatError::UnmatchedCloseBrace => write!(f, "unmatched '}}' in format string"),
            FormatError::MissingArgument { index } => {
                write!(f, "missing format argument at index {index}")
            }
            FormatError::UnusedArgument { index } => {
                write!(f, "unused format argument at index {index}")
            }
            FormatError::UnsupportedFormatType { type_char } => {
                write!(f, "unsupported format type '{type_char}'")
            }
            FormatError::TypeMismatch {
                format_type,
                value_type,
            } => {
                write!(
                    f,
                    "format type '{format_type}' cannot be applied to a {value_type} value"
                )
            }
        }
    }
}

/// Errors produced while expanding reader S-expressions into core
/// expressions (currently, only `for` requires expansion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandError {
    InvalidMetaSyntax,
    MetaPropertiesNotList,
    InvalidMetaProperty,
    InvalidMetaPropertyKey,
    DuplicatePropertyKey(String),
    InvalidForSyntax,
    InvalidForBinding,
    NonConstantForBound,
    NonIntegerForBound,
    ZeroForStep,
    ForExpansionLimitExceeded,
    ConstantDivisionByZero,
    ConstantRemainderByZero,
    ConstantIntegerOverflow,
    InvalidQuoteSyntax { got: usize },
    InvalidQuasiquoteSyntax { got: usize },
    InvalidUnquoteSyntax { got: usize },
    UnquoteOutsideQuasiquote,
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::InvalidMetaSyntax => write!(f, "malformed meta expression"),
            ExpandError::MetaPropertiesNotList => write!(f, "meta properties must be a list"),
            ExpandError::InvalidMetaProperty => {
                write!(f, "each meta property must be a two-element list")
            }
            ExpandError::InvalidMetaPropertyKey => write!(f, "meta property keys must be symbols"),
            ExpandError::DuplicatePropertyKey(key) => write!(f, "duplicate property key: {key}"),
            ExpandError::InvalidForSyntax => write!(f, "malformed for expression"),
            ExpandError::InvalidForBinding => write!(f, "malformed for binding"),
            ExpandError::NonConstantForBound => write!(
                f,
                "for bounds must be expansion-time integer constants; use loop for runtime ranges"
            ),
            ExpandError::NonIntegerForBound => {
                write!(f, "for bounds must be integer constants")
            }
            ExpandError::ZeroForStep => write!(f, "for step must not be zero"),
            ExpandError::ForExpansionLimitExceeded => {
                write!(f, "for expansion exceeded the maximum iteration limit")
            }
            ExpandError::ConstantDivisionByZero => {
                write!(
                    f,
                    "division by zero in a for expansion-time constant expression"
                )
            }
            ExpandError::ConstantRemainderByZero => {
                write!(
                    f,
                    "remainder by zero in a for expansion-time constant expression"
                )
            }
            ExpandError::ConstantIntegerOverflow => {
                write!(
                    f,
                    "integer overflow in a for expansion-time constant expression"
                )
            }
            ExpandError::InvalidQuoteSyntax { got } => {
                write!(f, "quote expects exactly 1 argument, got {got}")
            }
            ExpandError::InvalidQuasiquoteSyntax { got } => {
                write!(f, "quasiquote expects exactly 1 argument, got {got}")
            }
            ExpandError::InvalidUnquoteSyntax { got } => {
                write!(f, "unquote expects exactly 1 argument, got {got}")
            }
            ExpandError::UnquoteOutsideQuasiquote => {
                write!(f, "unquote may only be used inside quasiquote")
            }
        }
    }
}

/// Errors produced while lowering a core expression into IR (name
/// resolution, `def` placement, `break` target resolution, and resource
/// limits on the generated IR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    BreakOutsideRuntimeLoop,
    DefOutsideDirectTopLevel,
    ReservedDefinitionName(String),
    InvalidCoreForm,
    DuplicateParameter(String),
    DuplicateBinding(String),
    DuplicateLoopState(String),
    TooManyLocals,
    TooManyCaptures,
    TooManyFunctions,
    TooManyLoops,
    InvalidGensymSyntax { got: usize },
    UnboundGeneratedSymbol(String),
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LowerError::BreakOutsideRuntimeLoop => {
                write!(f, "break used outside of an enclosing runtime loop")
            }
            LowerError::DefOutsideDirectTopLevel => {
                write!(f, "def is only allowed at top level")
            }
            LowerError::ReservedDefinitionName(name) => write!(
                f,
                "'{name}' is a reserved special-form name and cannot be defined"
            ),
            LowerError::InvalidCoreForm => write!(f, "malformed core expression"),
            LowerError::DuplicateParameter(name) => write!(f, "duplicate parameter name: {name}"),
            LowerError::DuplicateBinding(name) => write!(f, "duplicate let binding name: {name}"),
            LowerError::DuplicateLoopState(name) => write!(f, "duplicate loop state name: {name}"),
            LowerError::TooManyLocals => write!(f, "too many local variables in one function"),
            LowerError::TooManyCaptures => write!(f, "too many captured variables in one closure"),
            LowerError::TooManyFunctions => write!(f, "too many functions in one module"),
            LowerError::TooManyLoops => write!(f, "too many runtime loops in one function"),
            LowerError::InvalidGensymSyntax { got } => {
                write!(f, "gensym expects 0 or 1 arguments, got {got}")
            }
            LowerError::UnboundGeneratedSymbol(name) => {
                write!(f, "unbound generated symbol: {name}")
            }
        }
    }
}

/// Errors produced while verifying that generated IR is internally
/// consistent before it is executed or handed to a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    InvalidFunctionId(FunctionId),
    InvalidGlobalId(GlobalId),
    InvalidLocalSlot(LocalSlot),
    InvalidCaptureSlot(CaptureSlot),
    CaptureCountMismatch { expected: u32, got: u32 },
    DuplicateLoopId(LoopId),
    BreakTargetNotEnclosing(LoopId),
    InvalidStateLoopUpdate,
    InvalidParameterCount,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::InvalidFunctionId(id) => write!(f, "invalid function id: {}", id.0),
            VerifyError::InvalidGlobalId(id) => write!(f, "invalid global id: {}", id.0),
            VerifyError::InvalidLocalSlot(slot) => write!(f, "invalid local slot: {}", slot.0),
            VerifyError::InvalidCaptureSlot(slot) => write!(f, "invalid capture slot: {}", slot.0),
            VerifyError::CaptureCountMismatch { expected, got } => write!(
                f,
                "closure capture count mismatch: expected {expected}, got {got}"
            ),
            VerifyError::DuplicateLoopId(id) => write!(f, "duplicate loop id: {}", id.0),
            VerifyError::BreakTargetNotEnclosing(id) => {
                write!(f, "break target loop id {} is not enclosing", id.0)
            }
            VerifyError::InvalidStateLoopUpdate => {
                write!(f, "state loop update does not match declared state slots")
            }
            VerifyError::InvalidParameterCount => {
                write!(f, "function parameter count exceeds its local count")
            }
        }
    }
}

/// Errors a backend (only `TextIrBackend` today) may report while
/// translating verified IR to its output form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    Unsupported(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Unsupported(what) => write!(f, "backend does not support: {what}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LispError {
    Lex(LexError),
    Parse(ParseError),
    Expand(ExpandError),
    Lower(LowerError),
    Verify(VerifyError),
    Eval(EvalError),
    Backend(BackendError),
    Macro(MacroExpandError),
}

impl fmt::Display for LispError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LispError::Lex(e) => write!(f, "lex error: {e}"),
            LispError::Parse(e) => write!(f, "parse error: {e}"),
            LispError::Expand(e) => write!(f, "expand error: {e}"),
            LispError::Lower(e) => write!(f, "lower error: {e}"),
            LispError::Verify(e) => write!(f, "verify error: {e}"),
            LispError::Eval(e) => write!(f, "eval error: {e}"),
            LispError::Backend(e) => write!(f, "backend error: {e}"),
            LispError::Macro(e) => write!(f, "macro expansion error: {e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroExpandError {
    InvalidMacroDefinition,
    InvalidMacroName,
    InvalidMacroParameterList,
    DuplicateMacroParameter(String),
    DuplicateMacroDefinition(String),
    ReservedMacroName(String),
    NestedMacroDefinition,
    GeneratedMacroDefinition,
    ForbiddenMacroOperation(String),
    MacroArityMismatch {
        name: String,
        expected: usize,
        got: usize,
    },
    MacroEvaluationFailed {
        name: String,
        cause: String,
        stack: Vec<String>,
    },
    MacroResultIsNotDatum {
        name: String,
        actual: String,
        stack: Vec<String>,
    },
    MacroExpansionDepthExceeded {
        stack: Vec<String>,
    },
    MacroExpansionBudgetExceeded {
        stack: Vec<String>,
    },
    InvalidDatumToExprConversion,
}

impl fmt::Display for MacroExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMacroDefinition => write!(f, "invalid defmacro definition"),
            Self::InvalidMacroName => write!(f, "macro name must be an interned symbol"),
            Self::InvalidMacroParameterList => {
                write!(f, "macro parameters must be a list of interned symbols")
            }
            Self::DuplicateMacroParameter(name) => write!(f, "duplicate macro parameter: {name}"),
            Self::DuplicateMacroDefinition(name) => write!(f, "duplicate macro definition: {name}"),
            Self::ReservedMacroName(name) => write!(
                f,
                "reserved special form cannot be redefined as a macro: {name}"
            ),
            Self::NestedMacroDefinition => write!(
                f,
                "defmacro is only allowed as a directly written top-level form"
            ),
            Self::GeneratedMacroDefinition => {
                write!(f, "a macro expansion may not generate defmacro")
            }
            Self::ForbiddenMacroOperation(name) => write!(
                f,
                "operation is not available during macro evaluation: {name}"
            ),
            Self::MacroArityMismatch {
                name,
                expected,
                got,
            } => write!(f, "macro {name} expects {expected} arguments, got {got}"),
            Self::MacroEvaluationFailed { name, cause, stack } => write!(
                f,
                "macro {name} evaluation failed (stack: {}): {cause}",
                stack.join(" -> ")
            ),
            Self::MacroResultIsNotDatum {
                name,
                actual,
                stack,
            } => write!(
                f,
                "macro {name} must return a Datum, got {actual} (stack: {})",
                stack.join(" -> ")
            ),
            Self::MacroExpansionDepthExceeded { stack } => write!(
                f,
                "macro expansion depth exceeded (stack: {})",
                stack.join(" -> ")
            ),
            Self::MacroExpansionBudgetExceeded { stack } => write!(
                f,
                "macro expansion invocation budget exceeded (stack: {})",
                stack.join(" -> ")
            ),
            Self::InvalidDatumToExprConversion => {
                write!(f, "macro result cannot be converted from Datum to syntax")
            }
        }
    }
}

impl From<MacroExpandError> for LispError {
    fn from(error: MacroExpandError) -> Self {
        Self::Macro(error)
    }
}

impl std::error::Error for LispError {}

impl From<LexError> for LispError {
    fn from(e: LexError) -> Self {
        LispError::Lex(e)
    }
}

impl From<ParseError> for LispError {
    fn from(e: ParseError) -> Self {
        LispError::Parse(e)
    }
}

impl From<ExpandError> for LispError {
    fn from(e: ExpandError) -> Self {
        LispError::Expand(e)
    }
}

impl From<LowerError> for LispError {
    fn from(e: LowerError) -> Self {
        LispError::Lower(e)
    }
}

impl From<VerifyError> for LispError {
    fn from(e: VerifyError) -> Self {
        LispError::Verify(e)
    }
}

impl From<EvalError> for LispError {
    fn from(e: EvalError) -> Self {
        LispError::Eval(e)
    }
}

impl From<BackendError> for LispError {
    fn from(e: BackendError) -> Self {
        LispError::Backend(e)
    }
}
