//! Expressions: the JSON input form, conversion to the stored
//! tree, the gate's type inference, and the interpreter's
//! evaluation.
//!
//! Typing is a define-time act (`type_of`): every leaf resolves
//! against the game's definitions, value references are acyclic
//! by construction (a value may only name values defined before
//! it, and redefinition with different inputs is a conflict), and
//! the result type is stored on the value record. Evaluation is
//! then total: arithmetic saturates, and the only refusals it
//! can still produce are for hand-edited records the gate never
//! saw — typed `TG-GAME-EXPR`, never a panic.

use crate::game::model::{ClockField, CmpOp, Expr, ExprType, Game, StatScope};
use crate::game::op::ExprInput;
use crate::game::state::{GameState, StatKey};
use crate::limits::{GAME_EXPR_DEPTH_MAX, GAME_EXPR_NODES_MAX};
use crate::refusal::{Refusal, RefusalClass, refuse};

fn expr_refusal<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::GameExpr, message)
}

fn unknown_kind<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::UnknownKind, message)
}

// ---- conversion ---------------------------------------------------

/// Converts an input expression to the stored tree, enforcing
/// the depth bound and resolving operator/field names.
pub fn expr_from_input(input: &ExprInput) -> Result<Expr, Refusal> {
    convert(input, 0)
}

fn convert(input: &ExprInput, depth: usize) -> Result<Expr, Refusal> {
    if depth > GAME_EXPR_DEPTH_MAX {
        return expr_refusal(format!(
            "expression nesting exceeds the depth bound {GAME_EXPR_DEPTH_MAX}"
        ));
    }
    let down = |inner: &ExprInput| convert(inner, depth + 1);
    let expr = match input {
        ExprInput::Add { left, right } => Expr::Add(Box::new(down(left)?), Box::new(down(right)?)),
        ExprInput::And { args } => Expr::And(convert_all(args, depth)?),
        ExprInput::BoolLit { value } => Expr::BoolLit(*value),
        ExprInput::Clamp { value, lo, hi } => Expr::Clamp(
            Box::new(down(value)?),
            Box::new(down(lo)?),
            Box::new(down(hi)?),
        ),
        ExprInput::Clock { field } => Expr::Clock(match field.as_str() {
            "day" => ClockField::Day,
            "days_remaining" => ClockField::DaysRemaining,
            "slot" => ClockField::Slot,
            "weekday" => ClockField::Weekday,
            other => {
                return unknown_kind(format!("clock field '{other}' is not known"));
            }
        }),
        ExprInput::Cmp { op, left, right } => {
            let op = match op.as_str() {
                "eq" => CmpOp::Eq,
                "ge" => CmpOp::Ge,
                "gt" => CmpOp::Gt,
                "le" => CmpOp::Le,
                "lt" => CmpOp::Lt,
                "ne" => CmpOp::Ne,
                other => {
                    return unknown_kind(format!("comparison operator '{other}' is not known"));
                }
            };
            Expr::Cmp(op, Box::new(down(left)?), Box::new(down(right)?))
        }
        ExprInput::Flag { flag } => Expr::Flag(flag.clone()),
        ExprInput::Lit { value } => Expr::Lit(*value),
        ExprInput::Max { left, right } => Expr::Max(Box::new(down(left)?), Box::new(down(right)?)),
        ExprInput::Min { left, right } => Expr::Min(Box::new(down(left)?), Box::new(down(right)?)),
        ExprInput::Mul { left, right } => Expr::Mul(Box::new(down(left)?), Box::new(down(right)?)),
        ExprInput::Neg { expr } => Expr::Neg(Box::new(down(expr)?)),
        ExprInput::Not { expr } => Expr::Not(Box::new(down(expr)?)),
        ExprInput::Or { args } => Expr::Or(convert_all(args, depth)?),
        ExprInput::Stat { stat, character } => Expr::Stat {
            character: character.clone(),
            stat: stat.clone(),
        },
        ExprInput::Sub { left, right } => Expr::Sub(Box::new(down(left)?), Box::new(down(right)?)),
        ExprInput::TierOf { stat, character } => Expr::TierOf {
            character: character.clone(),
            stat: stat.clone(),
        },
        ExprInput::Value { value } => Expr::Value(value.clone()),
    };
    Ok(expr)
}

fn convert_all(args: &[ExprInput], depth: usize) -> Result<Vec<Expr>, Refusal> {
    args.iter().map(|arg| convert(arg, depth + 1)).collect()
}

/// The input form of a stored expression (the validation sweep
/// synthesizes ops from stored records).
pub fn expr_to_input(expr: &Expr) -> ExprInput {
    match expr {
        Expr::Add(left, right) => ExprInput::Add {
            left: Box::new(expr_to_input(left)),
            right: Box::new(expr_to_input(right)),
        },
        Expr::And(args) => ExprInput::And {
            args: args.iter().map(expr_to_input).collect(),
        },
        Expr::BoolLit(value) => ExprInput::BoolLit { value: *value },
        Expr::Clamp(value, lo, hi) => ExprInput::Clamp {
            value: Box::new(expr_to_input(value)),
            lo: Box::new(expr_to_input(lo)),
            hi: Box::new(expr_to_input(hi)),
        },
        Expr::Clock(field) => ExprInput::Clock {
            field: match field {
                ClockField::Day => "day",
                ClockField::DaysRemaining => "days_remaining",
                ClockField::Slot => "slot",
                ClockField::Weekday => "weekday",
            }
            .to_string(),
        },
        Expr::Cmp(op, left, right) => ExprInput::Cmp {
            op: match op {
                CmpOp::Eq => "eq",
                CmpOp::Ge => "ge",
                CmpOp::Gt => "gt",
                CmpOp::Le => "le",
                CmpOp::Lt => "lt",
                CmpOp::Ne => "ne",
            }
            .to_string(),
            left: Box::new(expr_to_input(left)),
            right: Box::new(expr_to_input(right)),
        },
        Expr::Flag(flag) => ExprInput::Flag { flag: flag.clone() },
        Expr::Lit(value) => ExprInput::Lit { value: *value },
        Expr::Max(left, right) => ExprInput::Max {
            left: Box::new(expr_to_input(left)),
            right: Box::new(expr_to_input(right)),
        },
        Expr::Min(left, right) => ExprInput::Min {
            left: Box::new(expr_to_input(left)),
            right: Box::new(expr_to_input(right)),
        },
        Expr::Mul(left, right) => ExprInput::Mul {
            left: Box::new(expr_to_input(left)),
            right: Box::new(expr_to_input(right)),
        },
        Expr::Neg(inner) => ExprInput::Neg {
            expr: Box::new(expr_to_input(inner)),
        },
        Expr::Not(inner) => ExprInput::Not {
            expr: Box::new(expr_to_input(inner)),
        },
        Expr::Or(args) => ExprInput::Or {
            args: args.iter().map(expr_to_input).collect(),
        },
        Expr::Stat { character, stat } => ExprInput::Stat {
            stat: stat.clone(),
            character: character.clone(),
        },
        Expr::Sub(left, right) => ExprInput::Sub {
            left: Box::new(expr_to_input(left)),
            right: Box::new(expr_to_input(right)),
        },
        Expr::TierOf { character, stat } => ExprInput::TierOf {
            stat: stat.clone(),
            character: character.clone(),
        },
        Expr::Value(id) => ExprInput::Value { value: id.clone() },
    }
}

// ---- typing ---------------------------------------------------------

/// Resolves a stat leaf's scope discipline: character-scoped
/// stats are read per character (which must exist); game/party
/// stats are read without one.
pub fn check_stat_target(
    game: &Game,
    stat_id: &str,
    character: Option<&str>,
) -> Result<StatKey, Refusal> {
    let Some(stat) = game.stat(stat_id) else {
        return expr_refusal(format!("expression names unknown stat '{stat_id}'"));
    };
    match (stat.scope, character) {
        (StatScope::Character, Some(who)) => {
            if game.character(who).is_none() {
                return expr_refusal(format!(
                    "expression names unknown character '{who}' for stat '{stat_id}'"
                ));
            }
            Ok(StatKey {
                character: Some(who.to_string()),
                stat: stat_id.to_string(),
            })
        }
        (StatScope::Character, None) => expr_refusal(format!(
            "stat '{stat_id}' is character-scoped; the read must name a character"
        )),
        (_, Some(_)) => expr_refusal(format!(
            "stat '{stat_id}' is not character-scoped; the read must not name a character"
        )),
        (_, None) => Ok(StatKey {
            character: None,
            stat: stat_id.to_string(),
        }),
    }
}

/// Infers an expression's type against the game's definitions.
pub fn type_of(game: &Game, expr: &Expr) -> Result<ExprType, Refusal> {
    let mut nodes = 0usize;
    type_walk(game, expr, &mut nodes)
}

fn int(game: &Game, expr: &Expr, nodes: &mut usize) -> Result<(), Refusal> {
    match type_walk(game, expr, nodes)? {
        ExprType::Int => Ok(()),
        ExprType::Bool => expr_refusal("a boolean expression is used where an integer is required"),
    }
}

fn boolean(game: &Game, expr: &Expr, nodes: &mut usize) -> Result<(), Refusal> {
    match type_walk(game, expr, nodes)? {
        ExprType::Bool => Ok(()),
        ExprType::Int => expr_refusal("an integer expression is used where a boolean is required"),
    }
}

fn type_walk(game: &Game, expr: &Expr, nodes: &mut usize) -> Result<ExprType, Refusal> {
    *nodes += 1;
    if *nodes > GAME_EXPR_NODES_MAX {
        return expr_refusal(format!(
            "expression exceeds the node bound {GAME_EXPR_NODES_MAX}"
        ));
    }
    match expr {
        Expr::Add(left, right)
        | Expr::Max(left, right)
        | Expr::Min(left, right)
        | Expr::Mul(left, right)
        | Expr::Sub(left, right) => {
            int(game, left, nodes)?;
            int(game, right, nodes)?;
            Ok(ExprType::Int)
        }
        Expr::And(args) | Expr::Or(args) => {
            if args.is_empty() {
                return expr_refusal("a logic expression needs at least one argument");
            }
            for arg in args {
                boolean(game, arg, nodes)?;
            }
            Ok(ExprType::Bool)
        }
        Expr::BoolLit(_) => Ok(ExprType::Bool),
        Expr::Clamp(value, lo, hi) => {
            int(game, value, nodes)?;
            int(game, lo, nodes)?;
            int(game, hi, nodes)?;
            Ok(ExprType::Int)
        }
        Expr::Clock(field) => {
            let Some(clock) = &game.clock else {
                return expr_refusal("expression reads the clock, but the game defines none");
            };
            match field {
                ClockField::DaysRemaining if clock.day_limit.is_none() => {
                    expr_refusal("expression reads days_remaining, but the clock has no day_limit")
                }
                ClockField::Weekday if clock.days_per_week.is_none() => {
                    expr_refusal("expression reads weekday, but the clock has no days_per_week")
                }
                _ => Ok(ExprType::Int),
            }
        }
        Expr::Cmp(op, left, right) => {
            let left_type = type_walk(game, left, nodes)?;
            let right_type = type_walk(game, right, nodes)?;
            match op {
                CmpOp::Eq | CmpOp::Ne => {
                    if left_type != right_type {
                        return expr_refusal("a comparison's sides have different types");
                    }
                }
                _ => {
                    if left_type != ExprType::Int || right_type != ExprType::Int {
                        return expr_refusal("an ordered comparison needs integer sides");
                    }
                }
            }
            Ok(ExprType::Bool)
        }
        Expr::Flag(flag) => {
            if game.flag(flag).is_none() {
                return expr_refusal(format!("expression names unknown flag '{flag}'"));
            }
            Ok(ExprType::Bool)
        }
        Expr::Lit(_) => Ok(ExprType::Int),
        Expr::Neg(inner) => {
            int(game, inner, nodes)?;
            Ok(ExprType::Int)
        }
        Expr::Not(inner) => {
            boolean(game, inner, nodes)?;
            Ok(ExprType::Bool)
        }
        Expr::Stat { character, stat } => {
            check_stat_target(game, stat, character.as_deref())?;
            Ok(ExprType::Int)
        }
        Expr::TierOf { character, stat } => {
            check_stat_target(game, stat, character.as_deref())?;
            if game.tier(stat).is_none() {
                return expr_refusal(format!(
                    "tier_of names stat '{stat}', which has no tier definition"
                ));
            }
            Ok(ExprType::Int)
        }
        Expr::Value(id) => match game.value(id) {
            Some(def) => Ok(def.value_type),
            None => expr_refusal(format!("expression names unknown value '{id}'")),
        },
    }
}

// ---- evaluation -----------------------------------------------------

/// A runtime expression value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
}

/// Evaluates an expression against a run state. Total for
/// gate-typed expressions; still typed-refusing (never
/// panicking) on records the gate never saw.
pub fn eval(game: &Game, state: &GameState, expr: &Expr) -> Result<Value, Refusal> {
    eval_at(game, state, expr, 0)
}

fn as_int(value: Value) -> Result<i64, Refusal> {
    match value {
        Value::Int(n) => Ok(n),
        Value::Bool(_) => expr_refusal("a boolean value is used where an integer is required"),
    }
}

fn as_bool(value: Value) -> Result<bool, Refusal> {
    match value {
        Value::Bool(b) => Ok(b),
        Value::Int(_) => expr_refusal("an integer value is used where a boolean is required"),
    }
}

fn stat_value(game: &Game, state: &GameState, character: Option<&str>, stat: &str) -> i64 {
    let key = StatKey {
        character: character.map(str::to_string),
        stat: stat.to_string(),
    };
    match state.stats.get(&key) {
        Some(value) => *value,
        None => game.stat(stat).map_or(0, |def| def.default),
    }
}

fn eval_at(game: &Game, state: &GameState, expr: &Expr, depth: usize) -> Result<Value, Refusal> {
    if depth > GAME_EXPR_DEPTH_MAX {
        return expr_refusal("expression evaluation exceeds the depth bound");
    }
    let down = |inner: &Expr| eval_at(game, state, inner, depth + 1);
    match expr {
        Expr::Add(left, right) => Ok(Value::Int(
            as_int(down(left)?)?.saturating_add(as_int(down(right)?)?),
        )),
        Expr::And(args) => {
            for arg in args {
                if !as_bool(down(arg)?)? {
                    return Ok(Value::Bool(false));
                }
            }
            Ok(Value::Bool(true))
        }
        Expr::BoolLit(value) => Ok(Value::Bool(*value)),
        Expr::Clamp(value, lo, hi) => {
            let value = as_int(down(value)?)?;
            let lo = as_int(down(lo)?)?;
            let hi = as_int(down(hi)?)?;
            Ok(Value::Int(value.clamp(lo.min(hi), lo.max(hi))))
        }
        Expr::Clock(field) => {
            let clock = game
                .clock
                .as_ref()
                .ok_or_else(|| Refusal::new(RefusalClass::GameExpr, "the game defines no clock"))?;
            let value = match field {
                ClockField::Day => state.day,
                ClockField::DaysRemaining => clock
                    .day_limit
                    .map_or(0, |limit| i64::from(limit) - state.day),
                ClockField::Slot => i64::from(state.slot),
                ClockField::Weekday => clock
                    .days_per_week
                    .map_or(0, |cycle| (state.day - 1).rem_euclid(i64::from(cycle)) + 1),
            };
            Ok(Value::Int(value))
        }
        Expr::Cmp(op, left, right) => {
            let left = down(left)?;
            let right = down(right)?;
            let result = match (left, right) {
                (Value::Int(a), Value::Int(b)) => match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ge => a >= b,
                    CmpOp::Gt => a > b,
                    CmpOp::Le => a <= b,
                    CmpOp::Lt => a < b,
                    CmpOp::Ne => a != b,
                },
                (Value::Bool(a), Value::Bool(b)) => match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    _ => {
                        return expr_refusal("an ordered comparison is applied to booleans");
                    }
                },
                _ => return expr_refusal("a comparison's sides have different types"),
            };
            Ok(Value::Bool(result))
        }
        Expr::Flag(flag) => {
            let Some(def) = game.flag(flag) else {
                return expr_refusal(format!("expression names unknown flag '{flag}'"));
            };
            let value = match def.scope {
                crate::game::model::FlagScope::Globals => state.flags_globals.get(flag).copied(),
                crate::game::model::FlagScope::Run => state.flags_run.get(flag).copied(),
            }
            .unwrap_or(def.default);
            Ok(Value::Bool(value))
        }
        Expr::Lit(value) => Ok(Value::Int(*value)),
        Expr::Max(left, right) => Ok(Value::Int(as_int(down(left)?)?.max(as_int(down(right)?)?))),
        Expr::Min(left, right) => Ok(Value::Int(as_int(down(left)?)?.min(as_int(down(right)?)?))),
        Expr::Mul(left, right) => Ok(Value::Int(
            as_int(down(left)?)?.saturating_mul(as_int(down(right)?)?),
        )),
        Expr::Neg(inner) => Ok(Value::Int(as_int(down(inner)?)?.saturating_neg())),
        Expr::Not(inner) => Ok(Value::Bool(!as_bool(down(inner)?)?)),
        Expr::Or(args) => {
            for arg in args {
                if as_bool(down(arg)?)? {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        Expr::Stat { character, stat } => Ok(Value::Int(stat_value(
            game,
            state,
            character.as_deref(),
            stat,
        ))),
        Expr::Sub(left, right) => Ok(Value::Int(
            as_int(down(left)?)?.saturating_sub(as_int(down(right)?)?),
        )),
        Expr::TierOf { character, stat } => {
            let Some(tier) = game.tier(stat) else {
                return expr_refusal(format!(
                    "tier_of names stat '{stat}', which has no tier definition"
                ));
            };
            Ok(Value::Int(
                tier.band_of(stat_value(game, state, character.as_deref(), stat)) as i64,
            ))
        }
        Expr::Value(id) => {
            let Some(def) = game.value(id) else {
                return expr_refusal(format!("expression names unknown value '{id}'"));
            };
            eval_at(game, state, &def.expr, depth + 1)
        }
    }
}

/// Evaluates a named condition against a run state (the gate
/// for trigger firing).
pub fn eval_condition(game: &Game, state: &GameState, condition_id: &str) -> Result<bool, Refusal> {
    let Some(def) = game.condition(condition_id) else {
        return expr_refusal(format!("unknown condition '{condition_id}'"));
    };
    as_bool(eval(game, state, &def.expr)?)
}
