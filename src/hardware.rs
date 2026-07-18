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
pub struct HwModule {
    pub name: String,
    pub ports: Vec<HwPort>,
    pub assignments: Vec<HwAssignment>,
    pub registers: Vec<HwRegister>,
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
                        if next.replace(lower_expr(value, &lookup)?).is_some() {
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
        if assignments
            .iter()
            .any(|a: &HwAssignment| a.destination.id == id)
        {
            return Err(HardwareError::DuplicateAssignment(destination));
        };
        let value = lower_expr(value, &lookup)?;
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

fn lower_reset_value(
    expression: &Expr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
    ty: HwType,
) -> Result<HwExpr, HardwareError> {
    match lower_expr(expression, signals) {
        Ok(value) => Ok(value),
        Err(HardwareError::UntypedConstant) => {
            let ExprKind::Int(value) = expression.kind() else {
                return Err(HardwareError::UntypedConstant);
            };
            if !fits(*value, ty) {
                return Err(HardwareError::ConstantOutOfRange(*value));
            }
            Ok(HwExpr {
                kind: HwExprKind::Constant(*value),
                ty,
                properties: Properties::new(),
            })
        }
        Err(error) => Err(error),
    }
}
fn lower_core(
    core: &CoreExpr,
    signals: &HashMap<String, (HwSignalId, HwType, HwPortDirection)>,
) -> Result<HwExpr, HardwareError> {
    let props = core.properties().clone();
    match core.kind() {
        CoreExprKind::Symbol(name) => {
            let Some((id, ty, _)) = signals.get(name) else {
                return Err(HardwareError::UnknownSignal(name.clone()));
            };
            Ok(HwExpr {
                kind: HwExprKind::Reference(HwSignalRef { id: *id }),
                ty: *ty,
                properties: props,
            })
        }
        CoreExprKind::Int(value) => {
            let ty = type_from_properties(&props, "constant")
                .map_err(|_| HardwareError::UntypedConstant)?;
            if !fits(*value, ty) {
                return Err(HardwareError::ConstantOutOfRange(*value));
            };
            Ok(HwExpr {
                kind: HwExprKind::Constant(*value),
                ty,
                properties: props,
            })
        }
        CoreExprKind::List(items) => lower_application(items, props, signals),
        _ => Err(HardwareError::UnsupportedExpression),
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
        let then_expr = lower_core(yes, signals)?;
        let else_expr = lower_core(no, signals)?;
        if condition.ty
            != (HwType {
                width: 1,
                signed: false,
            })
        {
            return Err(HardwareError::InvalidCondition);
        };
        if then_expr.ty != else_expr.ty {
            return Err(HardwareError::TypeMismatch);
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
        let operand = lower_core(operand, signals)?;
        let ty = operand.ty;
        return Ok(HwExpr {
            kind: HwExprKind::Unary {
                op: HwUnaryOp::BitNot,
                operand: Box::new(operand),
            },
            ty,
            properties,
        });
    };
    let [lhs, rhs] = rest else {
        return Err(HardwareError::UnsupportedExpression);
    };
    let lhs = lower_core(lhs, signals)?;
    let rhs = lower_core(rhs, signals)?;
    if lhs.ty != rhs.ty {
        return Err(HardwareError::TypeMismatch);
    };
    let op = match name.as_str() {
        "+" => HwBinaryOp::Add,
        "-" => HwBinaryOp::Sub,
        "bit-and" => HwBinaryOp::BitAnd,
        "bit-or" => HwBinaryOp::BitOr,
        "bit-xor" => HwBinaryOp::BitXor,
        _ => return Err(HardwareError::UnsupportedExpression),
    };
    let ty = lhs.ty;
    Ok(HwExpr {
        kind: HwExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        ty,
        properties,
    })
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
        }
        for (index, port) in module.ports.iter().enumerate() {
            if port.ty.width == 0 {
                return Err(HardwareError::InvalidWidth(port.name.clone()));
            };
            if port.direction == HwPortDirection::Output && !assigned.contains(&HwSignalId(index)) {
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

fn collect_references(expr: &HwExpr, references: &mut Vec<HwSignalId>) {
    match &expr.kind {
        HwExprKind::Reference(reference) => references.push(reference.id),
        HwExprKind::Constant(_) => {}
        HwExprKind::Unary { operand, .. } => collect_references(operand, references),
        HwExprKind::Binary { lhs, rhs, .. } => {
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
