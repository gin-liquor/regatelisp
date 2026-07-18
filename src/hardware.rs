//! Backend-independent combinational hardware IR, lowering, verification, and
//! conservative SystemVerilog emission.
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ast::{Expr, ExprKind};
use crate::core::{CoreExpr, CoreExprKind};
use crate::expand::{self, ExpansionContext, ForConstantSource};
use crate::property::{Properties, PropertyValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    InvalidModule,
    DuplicateModule(String),
    DuplicatePort(String),
    InvalidPort,
    MissingWidth(String),
    InvalidWidth(String),
    InvalidSigned(String),
    UnknownSignal(String),
    InputAssignment(String),
    DuplicateAssignment(String),
    MissingAssignment(String),
    TypeMismatch,
    InvalidCondition,
    UntypedConstant,
    ConstantOutOfRange(i64),
    UnsupportedExpression,
    CombinationalLoop(String),
    InvalidIdentifier(String),
}
impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for HardwareError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwType {
    pub width: u32,
    pub signed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwPortDirection {
    Input,
    Output,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HwSignalId(pub usize);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwSignalRef {
    pub id: HwSignalId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwUnaryOp {
    BitNot,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwBinaryOp {
    Add,
    Sub,
    BitAnd,
    BitOr,
    BitXor,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwCompareOp {
    Eq,
    NotEq,
    LessThan,
    LessEqual,
    GreaterThan,
    GreaterEqual,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwExprKind {
    Reference(HwSignalRef),
    Constant(i64),
    Unary {
        op: HwUnaryOp,
        operand: Box<HwExpr>,
    },
    Binary {
        op: HwBinaryOp,
        lhs: Box<HwExpr>,
        rhs: Box<HwExpr>,
    },
    Compare {
        op: HwCompareOp,
        lhs: Box<HwExpr>,
        rhs: Box<HwExpr>,
    },
    Mux {
        condition: Box<HwExpr>,
        then_expr: Box<HwExpr>,
        else_expr: Box<HwExpr>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwExpr {
    pub kind: HwExprKind,
    pub ty: HwType,
    pub properties: Properties,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwPort {
    pub direction: HwPortDirection,
    pub name: String,
    pub ty: HwType,
    pub properties: Properties,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwAssignment {
    pub destination: HwSignalRef,
    pub value: HwExpr,
    pub properties: Properties,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwEdge {
    Rising,
    Falling,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwActiveLevel {
    High,
    Low,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwReset {
    pub signal: HwSignalId,
    pub active_level: HwActiveLevel,
    pub value: HwExpr,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwRegister {
    pub name: String,
    pub ty: HwType,
    pub clock: HwSignalId,
    pub edge: HwEdge,
    pub reset: Option<HwReset>,
    pub enable: Option<HwExpr>,
    pub next: HwExpr,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwStmt {
    Set {
        target: HwSignalId,
        value: HwExpr,
        properties: Properties,
    },
    If {
        condition: HwExpr,
        then_branch: Box<HwStmt>,
        else_branch: Option<Box<HwStmt>>,
        properties: Properties,
    },
    Block {
        statements: Vec<HwStmt>,
        properties: Properties,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwClockedBlock {
    pub clock: HwSignalId,
    pub edge: HwEdge,
    pub body: HwStmt,
    pub properties: Properties,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HwModule {
    pub name: String,
    pub ports: Vec<HwPort>,
    pub assignments: Vec<HwAssignment>,
    pub registers: Vec<HwRegister>,
    pub clocked_blocks: Vec<HwClockedBlock>,
    pub properties: Properties,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HwDesign {
    pub modules: Vec<HwModule>,
}

struct NoConstants;
impl ForConstantSource for NoConstants {
    fn integer_constant(&self, _: &str) -> Option<i64> {
        None
    }
}

pub fn lower_hardware_design(expressions: &[Expr]) -> Result<HwDesign, HardwareError> {
    let mut design = HwDesign::default();
    for expression in expressions {
        design.modules.push(lower_module(expression)?);
    }
    verify_hardware_design(&design)?;
    Ok(design)
}

fn list(expr: &Expr) -> Result<&[Expr], HardwareError> {
    if let ExprKind::List(items) = expr.kind() {
        Ok(items)
    } else {
        Err(HardwareError::InvalidModule)
    }
}
fn symbol(expr: &Expr) -> Result<&str, HardwareError> {
    if let ExprKind::Symbol(name) = expr.kind() {
        Ok(name)
    } else {
        Err(HardwareError::InvalidModule)
    }
}
fn valid_identifier(name: &str) -> bool {
    let mut c = name.chars();
    matches!(c.next(), Some(x) if x.is_ascii_alphabetic())
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}
fn lower_module(expr: &Expr) -> Result<HwModule, HardwareError> {
    let items = list(expr)?;
    let [head, name, rest @ ..] = items else {
        return Err(HardwareError::InvalidModule);
    };
    if symbol(head)? != "module" {
        return Err(HardwareError::InvalidModule);
    };
    let name = symbol(name)?.to_string();
    if !valid_identifier(&name) {
        return Err(HardwareError::InvalidIdentifier(name));
    }
    let mut ports = None;
    let mut register_forms = None;
    let mut direct_registers = Vec::new();
    let mut clocked_forms = Vec::new();
    let mut statements = Vec::new();
    for item in rest {
        let form = list(item)?;
        if matches!(form.first().map(|x| x.kind()), Some(ExprKind::Symbol(x)) if x=="ports") {
            if ports.replace(form).is_some() {
                return Err(HardwareError::InvalidModule);
            }
        } else if matches!(form.first().map(|x| x.kind()), Some(ExprKind::Symbol(x)) if x=="registers")
        {
            if register_forms.replace(form).is_some() {
                return Err(HardwareError::InvalidModule);
            }
        } else if matches!(form.first().map(|x| x.kind()), Some(ExprKind::Symbol(x)) if x=="register")
        {
            direct_registers.push(item);
        } else if matches!(form.first().map(|x| x.kind()), Some(ExprKind::Symbol(x)) if x=="clocked")
        {
            clocked_forms.push(item);
        } else {
            statements.push(item)
        }
    }
    let ports = ports.ok_or(HardwareError::InvalidModule)?;
    let mut result = Vec::new();
    let mut names = HashSet::new();
    for definition in &ports[1..] {
        let form = list(definition)?;
        let [direction, port] = form else {
            return Err(HardwareError::InvalidPort);
        };
        let direction = match symbol(direction)? {
            "input" => HwPortDirection::Input,
            "output" => HwPortDirection::Output,
            _ => return Err(HardwareError::InvalidPort),
        };
        let core = expand_port(port)?;
        let CoreExprKind::Symbol(port_name) = core.kind() else {
            return Err(HardwareError::InvalidPort);
        };
        if !valid_identifier(port_name) {
            return Err(HardwareError::InvalidIdentifier(port_name.clone()));
        };
        if !names.insert(port_name.to_ascii_lowercase()) {
            return Err(HardwareError::DuplicatePort(port_name.clone()));
        };
        let ty = type_from_properties(core.properties(), port_name)?;
        result.push(HwPort {
            direction,
            name: port_name.clone(),
            ty,
            properties: core.properties().clone(),
        });
    }
    let mut lookup: HashMap<_, _> = result
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), (HwSignalId(i), p.ty, p.direction)))
        .collect();
    let mut registers = Vec::new();
    if let Some(forms) = register_forms {
        for register in &forms[1..] {
            let form = list(register)?;
            let [head, register_name, type_expr, attributes @ ..] = form else {
                return Err(HardwareError::InvalidModule);
            };
            if symbol(head)? != "register" {
                return Err(HardwareError::InvalidModule);
            }
            let core = expand_port(type_expr)?;
            let CoreExprKind::Symbol(_) = core.kind() else {
                return Err(HardwareError::InvalidPort);
            };
            let register_name = symbol(register_name)?.to_string();
            if lookup.contains_key(&register_name) {
                return Err(HardwareError::DuplicatePort(register_name));
            }
            let ty = type_from_properties(core.properties(), &register_name)?;
            let id = HwSignalId(result.len() + registers.len());
            lookup.insert(register_name.clone(), (id, ty, HwPortDirection::Output));
            registers.push(HwRegister {
                name: register_name,
                ty,
                clock: HwSignalId(usize::MAX),
                edge: HwEdge::Rising,
                reset: None,
                enable: None,
                next: HwExpr {
                    kind: HwExprKind::Constant(0),
                    ty,
                    properties: Properties::new(),
                },
            });
            let _ = attributes;
        }
        for (register, form_expr) in registers.iter_mut().zip(&forms[1..]) {
            let form = list(form_expr)?;
            let attributes = &form[3..];
            let mut clock = None;
            let mut next = None;
            let mut reset = None;
            let mut enable = None;
            for attribute in attributes {
                let parts = list(attribute)?;
                let Some(head) = parts.first() else {
                    return Err(HardwareError::InvalidModule);
                };
                match symbol(head)? {
                    "clock" => {
                        let [_, signal, edge] = parts else {
                            return Err(HardwareError::InvalidModule);
                        };
                        let signal = symbol(signal)?;
                        let Some((id, ty, direction)) = lookup.get(signal).copied() else {
                            return Err(HardwareError::UnknownSignal(signal.into()));
                        };
                        if direction != HwPortDirection::Input
                            || ty
                                != (HwType {
                                    width: 1,
                                    signed: false,
                                })
                        {
                            return Err(HardwareError::InvalidCondition);
                        };
                        let edge = match symbol(edge)? {
                            "rising" => HwEdge::Rising,
                            "falling" => HwEdge::Falling,
                            _ => return Err(HardwareError::InvalidModule),
                        };
                        if clock.replace((id, edge)).is_some() {
                            return Err(HardwareError::InvalidModule);
                        }
                    }
                    "next" => {
                        let [_, value] = parts else {
                            return Err(HardwareError::InvalidModule);
                        };
                        if next
                            .replace(lower_expr_expected(value, &lookup, register.ty)?)
                            .is_some()
                        {
                            return Err(HardwareError::InvalidModule);
                        }
                    }
                    "enable" => {
                        let [_, value] = parts else {
                            return Err(HardwareError::InvalidModule);
                        };
                        if enable.replace(lower_expr(value, &lookup)?).is_some() {
                            return Err(HardwareError::InvalidModule);
                        }
                    }
                    "reset" => {
                        let [_, kind, signal, level, value] = parts else {
                            return Err(HardwareError::InvalidModule);
                        };
                        if symbol(kind)? != "sync" {
                            return Err(HardwareError::InvalidModule);
                        };
                        let signal = symbol(signal)?;
                        let Some((id, ty, _)) = lookup.get(signal).copied() else {
                            return Err(HardwareError::UnknownSignal(signal.into()));
                        };
                        if ty
                            != (HwType {
                                width: 1,
                                signed: false,
                            })
                        {
                            return Err(HardwareError::InvalidCondition);
                        };
                        let active_level = match symbol(level)? {
                            "high" => HwActiveLevel::High,
                            "low" => HwActiveLevel::Low,
                            _ => return Err(HardwareError::InvalidModule),
                        };
                        if reset
                            .replace(HwReset {
                                signal: id,
                                active_level,
                                value: lower_reset_value(value, &lookup, register.ty)?,
                            })
                            .is_some()
                        {
                            return Err(HardwareError::InvalidModule);
                        }
                    }
                    _ => return Err(HardwareError::InvalidModule),
                }
            }
            let (clock_id, edge) = clock.ok_or(HardwareError::InvalidModule)?;
            let next = next.ok_or(HardwareError::InvalidModule)?;
            if next.ty != register.ty {
                return Err(HardwareError::TypeMismatch);
            };
            if enable.as_ref().is_some_and(|x| {
                x.ty != (HwType {
                    width: 1,
                    signed: false,
                })
            }) {
                return Err(HardwareError::InvalidCondition);
            };
            if reset.as_ref().is_some_and(|x| x.value.ty != register.ty) {
                return Err(HardwareError::TypeMismatch);
            };
            register.clock = clock_id;
            register.edge = edge;
            register.next = next;
            register.enable = enable;
            register.reset = reset;
        }
    }
    let mut stage_five_registers = Vec::new();
    for form_expr in direct_registers {
        let form = list(form_expr)?;
        if form.len() == 2 {
            stage_five_registers.push(form_expr);
            continue;
        }
        let [head, name_expr, attributes @ ..] = form else {
            return Err(HardwareError::InvalidModule);
        };
        if symbol(head)? != "register" || attributes.is_empty() {
            return Err(HardwareError::InvalidModule);
        }
        let name = symbol(name_expr)?.to_string();
        if registers.iter().any(|register| register.name == name) {
            return Err(HardwareError::DuplicatePort(name));
        }
        let Some((id, ty, direction)) = lookup.get(&name).copied() else {
            return Err(HardwareError::UnknownSignal(name));
        };
        if direction != HwPortDirection::Output {
            return Err(HardwareError::InputAssignment(name));
        }
        let mut clock = None;
        let mut next = None;
        let mut reset = None;
        let mut enable = None;
        for attribute in attributes {
            let parts = list(attribute)?;
            let Some(head) = parts.first() else {
                return Err(HardwareError::InvalidModule);
            };
            match symbol(head)? {
                "clock" => {
                    let [_, signal, edge] = parts else {
                        return Err(HardwareError::InvalidModule);
                    };
                    let signal = symbol(signal)?;
                    let Some((clock_id, clock_ty, clock_direction)) = lookup.get(signal).copied()
                    else {
                        return Err(HardwareError::UnknownSignal(signal.into()));
                    };
                    if clock_direction != HwPortDirection::Input || clock_ty.width != 1 {
                        return Err(HardwareError::InvalidCondition);
                    };
                    let edge = match symbol(edge)? {
                        "rising" => HwEdge::Rising,
                        "falling" => HwEdge::Falling,
                        _ => return Err(HardwareError::InvalidModule),
                    };
                    if clock.replace((clock_id, edge)).is_some() {
                        return Err(HardwareError::InvalidModule);
                    }
                }
                "next" => {
                    let [_, value] = parts else {
                        return Err(HardwareError::InvalidModule);
                    };
                    if next
                        .replace(lower_expr_expected(value, &lookup, ty)?)
                        .is_some()
                    {
                        return Err(HardwareError::InvalidModule);
                    }
                }
                "enable" => {
                    let [_, value] = parts else {
                        return Err(HardwareError::InvalidModule);
                    };
                    if enable.replace(lower_expr(value, &lookup)?).is_some() {
                        return Err(HardwareError::InvalidModule);
                    }
                }
                "reset" => {
                    let (signal, value) = match parts {
                        [_, signal, value] => (signal, value),
                        [_, kind, signal, level, value]
                            if symbol(kind)? == "sync" && symbol(level)? == "high" =>
                        {
                            (signal, value)
                        }
                        _ => return Err(HardwareError::InvalidModule),
                    };
                    let signal = symbol(signal)?;
                    let Some((reset_id, reset_ty, _)) = lookup.get(signal).copied() else {
                        return Err(HardwareError::UnknownSignal(signal.into()));
                    };
                    if reset_ty.width != 1 {
                        return Err(HardwareError::InvalidCondition);
                    };
                    if reset
                        .replace(HwReset {
                            signal: reset_id,
                            active_level: HwActiveLevel::High,
                            value: lower_reset_value(value, &lookup, ty)?,
                        })
                        .is_some()
                    {
                        return Err(HardwareError::InvalidModule);
                    }
                }
                _ => return Err(HardwareError::InvalidModule),
            }
        }
        let (clock, edge) = clock.ok_or(HardwareError::InvalidModule)?;
        let next = next.ok_or(HardwareError::InvalidModule)?;
        if enable.as_ref().is_some_and(|value| value.ty.width != 1)
            || reset.as_ref().is_some_and(|value| value.value.ty != ty)
        {
            return Err(HardwareError::TypeMismatch);
        }
        registers.push(HwRegister {
            name,
            ty,
            clock,
            edge,
            reset,
            enable,
            next,
        });
        let _ = id;
    }
    for form_expr in stage_five_registers {
        let form = list(form_expr)?;
        let declaration = &form[1];
        let core = expand_port(declaration)?;
        let CoreExprKind::Symbol(name) = core.kind() else {
            return Err(HardwareError::InvalidPort);
        };
        let (id, ty) = if let Some((id, ty, direction)) = lookup.get(name).copied() {
            if direction != HwPortDirection::Output {
                return Err(HardwareError::InputAssignment(name.clone()));
            }
            (id, ty)
        } else {
            let ty = type_from_properties(core.properties(), name)?;
            let id = HwSignalId(result.len() + registers.len());
            lookup.insert(name.clone(), (id, ty, HwPortDirection::Output));
            (id, ty)
        };
        if registers.iter().any(|register| register.name == *name) {
            return Err(HardwareError::DuplicatePort(name.clone()));
        }
        registers.push(HwRegister {
            name: name.clone(),
            ty,
            clock: HwSignalId(usize::MAX),
            edge: HwEdge::Rising,
            reset: None,
            enable: None,
            next: HwExpr {
                kind: HwExprKind::Constant(0),
                ty,
                properties: Properties::new(),
            },
        });
        let _ = id;
    }
    let mut clocked_blocks = Vec::new();
    for form_expr in clocked_forms {
        clocked_blocks.push(lower_clocked(form_expr, &lookup, &registers, result.len())?);
    }
    let mut assignments = Vec::new();
    for statement in statements {
        let form = list(statement)?;
        let [head, destination, value] = form else {
            return Err(HardwareError::InvalidModule);
        };
        if symbol(head)? != "assign" {
            return Err(HardwareError::InvalidModule);
        };
        let destination = symbol(destination)?.to_string();
        let Some((id, ty, direction)) = lookup.get(&destination).copied() else {
            return Err(HardwareError::UnknownSignal(destination));
        };
        if direction != HwPortDirection::Output {
            return Err(HardwareError::InputAssignment(destination));
        };
        if registers
            .iter()
            .any(|register| register.name == destination)
        {
            return Err(HardwareError::DuplicateAssignment(destination));
        }
        if assignments
            .iter()
            .any(|a: &HwAssignment| a.destination.id == id)
        {
            return Err(HardwareError::DuplicateAssignment(destination));
        };
        let value = lower_expr_expected(value, &lookup, ty)?;
        if value.ty != ty {
            return Err(HardwareError::TypeMismatch);
        };
        assignments.push(HwAssignment {
            destination: HwSignalRef { id },
            value,
            properties: Properties::new(),
        });
    }
    Ok(HwModule {
        name,
        ports: result,
        assignments,
        registers,
        clocked_blocks,
        properties: expr.properties().clone(),
    })
}
fn expand_port(expr: &Expr) -> Result<CoreExpr, HardwareError> {
    let c = NoConstants;
    expand::expand(expr, &ExpansionContext::new(&c)).map_err(|_| HardwareError::InvalidPort)
}
fn type_from_properties(properties: &Properties, name: &str) -> Result<HwType, HardwareError> {
    let Some(PropertyValue::Int(width)) = properties.get("width") else {
        return Err(HardwareError::MissingWidth(name.into()));
    };
    let width = u32::try_from(*width)
        .ok()
        .filter(|x| *x > 0)
        .ok_or_else(|| HardwareError::InvalidWidth(name.into()))?;
    let signed = match properties.get("signed") {
        None => false,
        Some(PropertyValue::Bool(x)) => *x,
        _ => return Err(HardwareError::InvalidSigned(name.into())),
    };
    Ok(HwType { width, signed })
}
fn lower_expr(
    expr: &Expr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
) -> Result<HwExpr, HardwareError> {
    let c = NoConstants;
    let core = expand::expand(expr, &ExpansionContext::new(&c))
        .map_err(|_| HardwareError::UnsupportedExpression)?;
    lower_core(&core, signals)
}

fn lower_expr_expected(
    expr: &Expr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    expected: HwType,
) -> Result<HwExpr, HardwareError> {
    let c = NoConstants;
    let core = expand::expand(expr, &ExpansionContext::new(&c))
        .map_err(|_| HardwareError::UnsupportedExpression)?;
    lower_core_expected(&core, signals, expected)
}

fn lower_reset_value(
    expression: &Expr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    ty: HwType,
) -> Result<HwExpr, HardwareError> {
    lower_expr_expected(expression, signals, ty)
}
fn lower_core(
    core: &CoreExpr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
) -> Result<HwExpr, HardwareError> {
    lower_core_with_expected(core, signals, None)
}

fn lower_core_expected(
    core: &CoreExpr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    expected: HwType,
) -> Result<HwExpr, HardwareError> {
    lower_core_with_expected(core, signals, Some(expected))
}

fn lower_core_with_expected(
    core: &CoreExpr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    expected: Option<HwType>,
) -> Result<HwExpr, HardwareError> {
    let props = core.properties().clone();
    match core.kind() {
        CoreExprKind::Symbol(name) => {
            let Some((id, ty, _)) = signals.get(name) else {
                return Err(HardwareError::UnknownSignal(name.clone()));
            };
            let value = HwExpr {
                kind: HwExprKind::Reference(HwSignalRef { id: *id }),
                ty: *ty,
                properties: props,
            };
            require_type(value, expected)
        }
        CoreExprKind::Int(value) => {
            let ty = match type_from_properties(&props, "constant") {
                Ok(ty) => ty,
                Err(_) => expected.ok_or(HardwareError::UntypedConstant)?,
            };
            if !fits(*value, ty) {
                return Err(HardwareError::ConstantOutOfRange(*value));
            };
            require_type(
                HwExpr {
                    kind: HwExprKind::Constant(*value),
                    ty,
                    properties: props,
                },
                expected,
            )
        }
        CoreExprKind::List(items) => lower_application(items, props, signals, expected),
        _ => Err(HardwareError::UnsupportedExpression),
    }
}
fn require_type(value: HwExpr, expected: Option<HwType>) -> Result<HwExpr, HardwareError> {
    if expected.is_none_or(|expected| value.ty == expected) {
        Ok(value)
    } else {
        Err(HardwareError::TypeMismatch)
    }
}
fn fits(value: i64, ty: HwType) -> bool {
    if ty.width >= 63 {
        return true;
    };
    if ty.signed {
        let n = 1_i64 << (ty.width - 1);
        (-n..n).contains(&value)
    } else {
        value >= 0 && value < (1_i64 << ty.width)
    }
}
fn lower_application(
    items: &[CoreExpr],
    properties: Properties,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    expected: Option<HwType>,
) -> Result<HwExpr, HardwareError> {
    let [head, rest @ ..] = items else {
        return Err(HardwareError::UnsupportedExpression);
    };
    let CoreExprKind::Symbol(name) = head.kind() else {
        return Err(HardwareError::UnsupportedExpression);
    };
    if name == "if" {
        let [condition, yes, no] = rest else {
            return Err(HardwareError::UnsupportedExpression);
        };
        let condition = lower_core(condition, signals)?;
        if condition.ty.width != 1 {
            return Err(HardwareError::InvalidCondition);
        };
        let (then_expr, else_expr) = if let Some(expected) = expected {
            (
                lower_core_expected(yes, signals, expected)?,
                lower_core_expected(no, signals, expected)?,
            )
        } else {
            match lower_core(yes, signals) {
                Ok(then_expr) => (
                    then_expr.clone(),
                    lower_core_expected(no, signals, then_expr.ty)?,
                ),
                Err(HardwareError::UntypedConstant) => {
                    let else_expr = lower_core(no, signals)?;
                    (lower_core_expected(yes, signals, else_expr.ty)?, else_expr)
                }
                Err(error) => return Err(error),
            }
        };
        let ty = then_expr.ty;
        return Ok(HwExpr {
            kind: HwExprKind::Mux {
                condition: Box::new(condition),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            },
            ty,
            properties,
        });
    };
    if name == "bit-not" {
        let [operand] = rest else {
            return Err(HardwareError::UnsupportedExpression);
        };
        let operand = match expected {
            Some(expected) => lower_core_expected(operand, signals, expected)?,
            None => lower_core(operand, signals)?,
        };
        let ty = operand.ty;
        return require_type(
            HwExpr {
                kind: HwExprKind::Unary {
                    op: HwUnaryOp::BitNot,
                    operand: Box::new(operand),
                },
                ty,
                properties,
            },
            expected,
        );
    };
    let [lhs, rhs] = rest else {
        return Err(HardwareError::UnsupportedExpression);
    };
    let comparison = match name.as_str() {
        "=" => Some(HwCompareOp::Eq),
        "!=" => Some(HwCompareOp::NotEq),
        "<" => Some(HwCompareOp::LessThan),
        "<=" => Some(HwCompareOp::LessEqual),
        ">" => Some(HwCompareOp::GreaterThan),
        ">=" => Some(HwCompareOp::GreaterEqual),
        _ => None,
    };
    let (lhs, rhs) = match lower_core(lhs, signals) {
        Ok(lhs) => (lhs.clone(), lower_core_expected(rhs, signals, lhs.ty)?),
        Err(HardwareError::UntypedConstant) if comparison.is_some() => {
            let rhs = lower_core(rhs, signals)?;
            (lower_core_expected(lhs, signals, rhs.ty)?, rhs)
        }
        Err(HardwareError::UntypedConstant) => return Err(HardwareError::UntypedConstant),
        Err(error) => return Err(error),
    };
    if lhs.ty != rhs.ty {
        return Err(HardwareError::TypeMismatch);
    };
    if let Some(op) = comparison {
        if lhs.ty.signed {
            return Err(HardwareError::UnsupportedExpression);
        }
        return require_type(
            HwExpr {
                kind: HwExprKind::Compare {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                ty: HwType {
                    width: 1,
                    signed: false,
                },
                properties,
            },
            expected,
        );
    }
    let op = match name.as_str() {
        "+" => HwBinaryOp::Add,
        "-" => HwBinaryOp::Sub,
        "bit-and" => HwBinaryOp::BitAnd,
        "bit-or" => HwBinaryOp::BitOr,
        "bit-xor" => HwBinaryOp::BitXor,
        _ => return Err(HardwareError::UnsupportedExpression),
    };
    let ty = lhs.ty;
    require_type(
        HwExpr {
            kind: HwExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty,
            properties,
        },
        expected,
    )
}

fn lower_clocked(
    expression: &Expr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    registers: &[HwRegister],
    port_count: usize,
) -> Result<HwClockedBlock, HardwareError> {
    let form = list(expression)?;
    let [head, clock_form, statements @ ..] = form else {
        return Err(HardwareError::InvalidModule);
    };
    if symbol(head)? != "clocked" || statements.is_empty() {
        return Err(HardwareError::InvalidModule);
    }
    let clock_parts = list(clock_form)?;
    let [clock_head, clock_name, edge_name] = clock_parts else {
        return Err(HardwareError::InvalidModule);
    };
    if symbol(clock_head)? != "clock" {
        return Err(HardwareError::InvalidModule);
    }
    let clock_name = symbol(clock_name)?;
    let Some((clock, ty, direction)) = signals.get(clock_name).copied() else {
        return Err(HardwareError::UnknownSignal(clock_name.into()));
    };
    if direction != HwPortDirection::Input
        || ty
            != (HwType {
                width: 1,
                signed: false,
            })
    {
        return Err(HardwareError::InvalidCondition);
    }
    let edge = match symbol(edge_name)? {
        "rising" => HwEdge::Rising,
        "falling" => HwEdge::Falling,
        _ => return Err(HardwareError::InvalidModule),
    };
    Ok(HwClockedBlock {
        clock,
        edge,
        body: lower_stmt_list(statements, signals, registers, port_count)?,
        properties: expression.properties().clone(),
    })
}

fn lower_stmt_list(
    items: &[Expr],
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    registers: &[HwRegister],
    port_count: usize,
) -> Result<HwStmt, HardwareError> {
    if items.len() == 1 {
        lower_stmt(&items[0], signals, registers, port_count)
    } else {
        Ok(HwStmt::Block {
            statements: items
                .iter()
                .map(|item| lower_stmt(item, signals, registers, port_count))
                .collect::<Result<_, _>>()?,
            properties: Properties::new(),
        })
    }
}

fn lower_stmt(
    expr: &Expr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    registers: &[HwRegister],
    port_count: usize,
) -> Result<HwStmt, HardwareError> {
    let form = list(expr)?;
    let Some(head) = form.first() else {
        return Err(HardwareError::InvalidModule);
    };
    match symbol(head)? {
        "set" => {
            let [_, target, value] = form else {
                return Err(HardwareError::InvalidModule);
            };
            let name = symbol(target)?;
            let Some((id, ty, _)) = signals.get(name).copied() else {
                return Err(HardwareError::UnknownSignal(name.into()));
            };
            if !registers.iter().any(|r| r.name == name)
                || (id.0 < port_count && !registers.iter().any(|r| r.name == name))
            {
                return Err(HardwareError::InputAssignment(name.into()));
            }
            Ok(HwStmt::Set {
                target: id,
                value: lower_expr_expected(value, signals, ty)?,
                properties: expr.properties().clone(),
            })
        }
        "do" => {
            if form.len() < 2 {
                return Err(HardwareError::InvalidModule);
            }
            Ok(HwStmt::Block {
                statements: form[1..]
                    .iter()
                    .map(|item| lower_stmt(item, signals, registers, port_count))
                    .collect::<Result<_, _>>()?,
                properties: expr.properties().clone(),
            })
        }
        "if" => {
            let ([_, condition, then_branch] | [_, condition, then_branch, _]) = form else {
                return Err(HardwareError::InvalidModule);
            };
            let condition = lower_expr(condition, signals)?;
            if condition.ty
                != (HwType {
                    width: 1,
                    signed: false,
                })
            {
                return Err(HardwareError::InvalidCondition);
            }
            let else_branch = if form.len() == 4 {
                Some(Box::new(lower_stmt(
                    &form[3], signals, registers, port_count,
                )?))
            } else {
                None
            };
            Ok(HwStmt::If {
                condition,
                then_branch: Box::new(lower_stmt(then_branch, signals, registers, port_count)?),
                else_branch,
                properties: expr.properties().clone(),
            })
        }
        _ => Err(HardwareError::InvalidModule),
    }
}

pub fn verify_hardware_design(design: &HwDesign) -> Result<(), HardwareError> {
    let mut modules = HashSet::new();
    for module in &design.modules {
        if !modules.insert(module.name.clone()) {
            return Err(HardwareError::DuplicateModule(module.name.clone()));
        };
        let mut assigned = HashSet::new();
        for assignment in &module.assignments {
            let port = module
                .ports
                .get(assignment.destination.id.0)
                .ok_or(HardwareError::InvalidModule)?;
            if port.direction != HwPortDirection::Output {
                return Err(HardwareError::InputAssignment(port.name.clone()));
            };
            if !assigned.insert(assignment.destination.id) {
                return Err(HardwareError::DuplicateAssignment(port.name.clone()));
            };
            if assignment.value.ty != port.ty {
                return Err(HardwareError::TypeMismatch);
            }
            verify_hardware_expr(&assignment.value, module)?;
        }
        for register in &module.registers {
            if register.clock.0 == usize::MAX {
                continue;
            }
            if register.ty.width == 0 {
                return Err(HardwareError::InvalidWidth(register.name.clone()));
            }
            if signal_type(module, register.clock).is_none_or(|ty| ty.width != 1) {
                return Err(HardwareError::InvalidCondition);
            }
            if register.next.ty != register.ty {
                return Err(HardwareError::TypeMismatch);
            }
            verify_hardware_expr(&register.next, module)?;
            if let Some(reset) = &register.reset {
                if signal_type(module, reset.signal).is_none_or(|ty| ty.width != 1)
                    || reset.value.ty != register.ty
                {
                    return Err(HardwareError::TypeMismatch);
                }
                verify_hardware_expr(&reset.value, module)?;
            }
            if let Some(enable) = &register.enable {
                if enable.ty.width != 1 {
                    return Err(HardwareError::InvalidCondition);
                }
                verify_hardware_expr(enable, module)?;
            }
        }
        let mut clocked_drivers = HashSet::new();
        for block in &module.clocked_blocks {
            let Some(clock_ty) = signal_type(module, block.clock) else {
                return Err(HardwareError::InvalidCondition);
            };
            if block.clock.0 >= module.ports.len()
                || module.ports[block.clock.0].direction != HwPortDirection::Input
                || clock_ty
                    != (HwType {
                        width: 1,
                        signed: false,
                    })
            {
                return Err(HardwareError::InvalidCondition);
            }
            let mut path_updates = HashSet::new();
            verify_stmt(&block.body, module, &mut path_updates)?;
            for target in path_updates {
                if !clocked_drivers.insert(target) {
                    return Err(HardwareError::DuplicateAssignment(
                        signal_name(module, target).into(),
                    ));
                }
                if assigned.contains(&target) {
                    return Err(HardwareError::DuplicateAssignment(
                        signal_name(module, target).into(),
                    ));
                }
            }
        }
        for (index, port) in module.ports.iter().enumerate() {
            if port.ty.width == 0 {
                return Err(HardwareError::InvalidWidth(port.name.clone()));
            };
            if port.direction == HwPortDirection::Output
                && !assigned.contains(&HwSignalId(index))
                && !module
                    .registers
                    .iter()
                    .any(|register| register.name == port.name)
            {
                return Err(HardwareError::MissingAssignment(port.name.clone()));
            }
        }
        let assignment_by_output: HashMap<_, _> = module
            .assignments
            .iter()
            .map(|assignment| (assignment.destination.id, assignment))
            .collect();
        fn visit(
            signal: HwSignalId,
            module: &HwModule,
            assignments: &HashMap<HwSignalId, &HwAssignment>,
            visiting: &mut HashSet<HwSignalId>,
            complete: &mut HashSet<HwSignalId>,
        ) -> Result<(), HardwareError> {
            if complete.contains(&signal) {
                return Ok(());
            }
            if !visiting.insert(signal) {
                return Err(HardwareError::CombinationalLoop(
                    module.ports[signal.0].name.clone(),
                ));
            }
            if let Some(assignment) = assignments.get(&signal) {
                let mut refs = Vec::new();
                collect_references(&assignment.value, &mut refs);
                for reference in refs {
                    if reference.0 < module.ports.len()
                        && module.ports[reference.0].direction == HwPortDirection::Output
                    {
                        visit(reference, module, assignments, visiting, complete)?;
                    }
                }
            }
            visiting.remove(&signal);
            complete.insert(signal);
            Ok(())
        }
        let mut visiting = HashSet::new();
        let mut complete = HashSet::new();
        for assignment in &module.assignments {
            visit(
                assignment.destination.id,
                module,
                &assignment_by_output,
                &mut visiting,
                &mut complete,
            )?;
        }
    }
    Ok(())
}

fn verify_stmt(
    statement: &HwStmt,
    module: &HwModule,
    updates: &mut HashSet<HwSignalId>,
) -> Result<(), HardwareError> {
    match statement {
        HwStmt::Set { target, value, .. } => {
            let Some(target_ty) = signal_type(module, *target) else {
                return Err(HardwareError::TypeMismatch);
            };
            if !module
                .registers
                .iter()
                .any(|register| register.name == signal_name(module, *target))
                || value.ty != target_ty
            {
                return Err(HardwareError::TypeMismatch);
            }
            verify_hardware_expr(value, module)?;
            if !updates.insert(*target) {
                return Err(HardwareError::DuplicateAssignment(
                    signal_name(module, *target).into(),
                ));
            }
            Ok(())
        }
        HwStmt::Block { statements, .. } => {
            if statements.is_empty() {
                return Err(HardwareError::InvalidModule);
            }
            for statement in statements {
                verify_stmt(statement, module, updates)?;
            }
            Ok(())
        }
        HwStmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            if condition.ty
                != (HwType {
                    width: 1,
                    signed: false,
                })
            {
                return Err(HardwareError::InvalidCondition);
            }
            verify_hardware_expr(condition, module)?;
            let mut then_updates = updates.clone();
            verify_stmt(then_branch, module, &mut then_updates)?;
            let mut else_updates = updates.clone();
            if let Some(else_branch) = else_branch {
                verify_stmt(else_branch, module, &mut else_updates)?;
            }
            updates.extend(then_updates);
            updates.extend(else_updates);
            Ok(())
        }
    }
}
fn emit_stmt(statement: &HwStmt, module: &HwModule, indent: usize, out: &mut String) {
    let padding = "    ".repeat(indent);
    match statement {
        HwStmt::Set { target, value, .. } => out.push_str(&format!(
            "{padding}{} <= {};\n",
            signal_name(module, *target),
            emit_expr(value, module)
        )),
        HwStmt::Block { statements, .. } => {
            for statement in statements {
                emit_stmt(statement, module, indent, out);
            }
        }
        HwStmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            out.push_str(&format!(
                "{padding}if ({}) begin\n",
                emit_expr(condition, module)
            ));
            emit_stmt(then_branch, module, indent + 1, out);
            out.push_str(&format!("{padding}end"));
            if let Some(else_branch) = else_branch {
                out.push_str(" else begin\n");
                emit_stmt(else_branch, module, indent + 1, out);
                out.push_str(&format!("{padding}end"));
            }
            out.push('\n');
        }
    }
}

fn collect_references(expr: &HwExpr, references: &mut Vec<HwSignalId>) {
    match &expr.kind {
        HwExprKind::Reference(reference) => references.push(reference.id),
        HwExprKind::Constant(_) => {}
        HwExprKind::Unary { operand, .. } => collect_references(operand, references),
        HwExprKind::Binary { lhs, rhs, .. } => {
            collect_references(lhs, references);
            collect_references(rhs, references);
        }
        HwExprKind::Compare { lhs, rhs, .. } => {
            collect_references(lhs, references);
            collect_references(rhs, references);
        }
        HwExprKind::Mux {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_references(condition, references);
            collect_references(then_expr, references);
            collect_references(else_expr, references);
        }
    }
}

fn signal_type(module: &HwModule, id: HwSignalId) -> Option<HwType> {
    module.ports.get(id.0).map(|port| port.ty).or_else(|| {
        module
            .registers
            .get(id.0.checked_sub(module.ports.len())?)
            .map(|r| r.ty)
    })
}

fn verify_hardware_expr(expr: &HwExpr, module: &HwModule) -> Result<(), HardwareError> {
    if expr.ty.width == 0 {
        return Err(HardwareError::InvalidWidth("expression".into()));
    }
    match &expr.kind {
        HwExprKind::Reference(reference) => {
            if signal_type(module, reference.id) == Some(expr.ty) {
                Ok(())
            } else {
                Err(HardwareError::TypeMismatch)
            }
        }
        HwExprKind::Constant(value) if fits(*value, expr.ty) => Ok(()),
        HwExprKind::Constant(value) => Err(HardwareError::ConstantOutOfRange(*value)),
        HwExprKind::Unary { operand, .. } => {
            verify_hardware_expr(operand, module)?;
            if operand.ty == expr.ty {
                Ok(())
            } else {
                Err(HardwareError::TypeMismatch)
            }
        }
        HwExprKind::Binary { lhs, rhs, .. } => {
            verify_hardware_expr(lhs, module)?;
            verify_hardware_expr(rhs, module)?;
            if lhs.ty == rhs.ty && lhs.ty == expr.ty {
                Ok(())
            } else {
                Err(HardwareError::TypeMismatch)
            }
        }
        HwExprKind::Compare { lhs, rhs, .. } => {
            verify_hardware_expr(lhs, module)?;
            verify_hardware_expr(rhs, module)?;
            if !lhs.ty.signed
                && lhs.ty == rhs.ty
                && expr.ty
                    == (HwType {
                        width: 1,
                        signed: false,
                    })
            {
                Ok(())
            } else {
                Err(HardwareError::TypeMismatch)
            }
        }
        HwExprKind::Mux {
            condition,
            then_expr,
            else_expr,
        } => {
            verify_hardware_expr(condition, module)?;
            verify_hardware_expr(then_expr, module)?;
            verify_hardware_expr(else_expr, module)?;
            if condition.ty.width == 1 && then_expr.ty == else_expr.ty && expr.ty == then_expr.ty {
                Ok(())
            } else {
                Err(HardwareError::TypeMismatch)
            }
        }
    }
}

pub fn emit_systemverilog(design: &HwDesign) -> Result<String, HardwareError> {
    verify_hardware_design(design)?;
    let mut out = String::new();
    for (n, module) in design.modules.iter().enumerate() {
        if n > 0 {
            out.push('\n')
        };
        out.push_str(&format!("module {} (\n", module.name));
        for (i, port) in module.ports.iter().enumerate() {
            out.push_str("    ");
            out.push_str(match port.direction {
                HwPortDirection::Input => "input  wire",
                HwPortDirection::Output
                    if module
                        .registers
                        .iter()
                        .any(|register| register.name == port.name) =>
                {
                    "output logic"
                }
                HwPortDirection::Output => "output wire",
            });
            if port.ty.signed {
                out.push_str(" signed")
            };
            if port.ty.width > 1 {
                out.push_str(&format!(" [{}:0]", port.ty.width - 1))
            };
            out.push_str(&format!(
                " {}{}\n",
                port.name,
                if i + 1 == module.ports.len() { "" } else { "," }
            ))
        }
        out.push_str(");\n\n");
        for register in &module.registers {
            if module
                .ports
                .iter()
                .any(|port| port.direction == HwPortDirection::Output && port.name == register.name)
            {
                continue;
            }
            out.push_str("logic");
            if register.ty.signed {
                out.push_str(" signed");
            }
            if register.ty.width > 1 {
                out.push_str(&format!(" [{}:0]", register.ty.width - 1));
            }
            out.push_str(&format!(" {};\n", register.name));
        }
        if !module.registers.is_empty() {
            out.push('\n');
        }
        for register in &module.registers {
            if register.clock.0 == usize::MAX {
                continue;
            }
            out.push_str(&format!(
                "always_ff @({} {}) begin\n",
                match register.edge {
                    HwEdge::Rising => "posedge",
                    HwEdge::Falling => "negedge",
                },
                signal_name(module, register.clock)
            ));
            if let Some(reset) = &register.reset {
                let condition = match reset.active_level {
                    HwActiveLevel::High => signal_name(module, reset.signal).to_string(),
                    HwActiveLevel::Low => format!("!{}", signal_name(module, reset.signal)),
                };
                out.push_str(&format!(
                    "    if ({condition}) begin\n        {} <= {};\n    end",
                    register.name,
                    emit_expr(&reset.value, module)
                ));
                if let Some(enable) = &register.enable {
                    out.push_str(&format!(
                        " else if ({}) begin\n        {} <= {};\n    end",
                        emit_expr(enable, module),
                        register.name,
                        emit_expr(&register.next, module)
                    ));
                } else {
                    out.push_str(&format!(
                        " else begin\n        {} <= {};\n    end",
                        register.name,
                        emit_expr(&register.next, module)
                    ));
                }
                out.push('\n');
            } else if let Some(enable) = &register.enable {
                out.push_str(&format!(
                    "    if ({}) begin\n        {} <= {};\n    end\n",
                    emit_expr(enable, module),
                    register.name,
                    emit_expr(&register.next, module)
                ));
            } else {
                out.push_str(&format!(
                    "    {} <= {};\n",
                    register.name,
                    emit_expr(&register.next, module)
                ));
            }
            out.push_str("end\n\n");
        }
        for block in &module.clocked_blocks {
            out.push_str(&format!(
                "always_ff @({} {}) begin\n",
                match block.edge {
                    HwEdge::Rising => "posedge",
                    HwEdge::Falling => "negedge",
                },
                signal_name(module, block.clock)
            ));
            emit_stmt(&block.body, module, 1, &mut out);
            out.push_str("end\n\n");
        }
        for assignment in &module.assignments {
            out.push_str(&format!(
                "assign {} = {};\n",
                signal_name(module, assignment.destination.id),
                emit_expr(&assignment.value, module)
            ))
        }
        out.push_str("\nendmodule\n")
    }
    Ok(out)
}
fn emit_expr(expr: &HwExpr, module: &HwModule) -> String {
    match &expr.kind {
        HwExprKind::Reference(r) => signal_name(module, r.id).to_string(),
        HwExprKind::Constant(v) => {
            if *v < 0 {
                format!(
                    "-{}'{}d{}",
                    expr.ty.width,
                    if expr.ty.signed { "s" } else { "" },
                    v.unsigned_abs()
                )
            } else {
                format!(
                    "{}'{}d{}",
                    expr.ty.width,
                    if expr.ty.signed { "s" } else { "" },
                    v
                )
            }
        }
        HwExprKind::Unary { operand, .. } => format!("(~{})", emit_expr(operand, module)),
        HwExprKind::Binary { op, lhs, rhs } => format!(
            "({} {} {})",
            emit_expr(lhs, module),
            match op {
                HwBinaryOp::Add => "+",
                HwBinaryOp::Sub => "-",
                HwBinaryOp::BitAnd => "&",
                HwBinaryOp::BitOr => "|",
                HwBinaryOp::BitXor => "^",
            },
            emit_expr(rhs, module)
        ),
        HwExprKind::Compare { op, lhs, rhs } => format!(
            "({} {} {})",
            emit_expr(lhs, module),
            match op {
                HwCompareOp::Eq => "==",
                HwCompareOp::NotEq => "!=",
                HwCompareOp::LessThan => "<",
                HwCompareOp::LessEqual => "<=",
                HwCompareOp::GreaterThan => ">",
                HwCompareOp::GreaterEqual => ">=",
            },
            emit_expr(rhs, module)
        ),
        HwExprKind::Mux {
            condition,
            then_expr,
            else_expr,
        } => format!(
            "({} ? {} : {})",
            emit_expr(condition, module),
            emit_expr(then_expr, module),
            emit_expr(else_expr, module)
        ),
    }
}

fn signal_name(module: &HwModule, id: HwSignalId) -> &str {
    if id.0 < module.ports.len() {
        &module.ports[id.0].name
    } else {
        &module.registers[id.0 - module.ports.len()].name
    }
}
