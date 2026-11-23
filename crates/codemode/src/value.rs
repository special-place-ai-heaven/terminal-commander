// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Bounded conversion between `serde_json::Value` and `monty_types::MontyObject`
//! / `CallArgs`, enforcing maximum nesting depth and decoded byte size.

use monty_types::{CallArgs, ExcType, MontyException, MontyObject, ObjectRef};
use serde_json::{Map, Number, Value};

/// Maximum JSON / Python container nesting depth accepted across the sandbox boundary.
pub const MAX_VALUE_DEPTH: usize = 126;
/// Maximum cumulative payload bytes converted into or out of a single Monty value.
pub const MAX_VALUE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    DepthExceeded { max_depth: usize },
    SizeExceeded { max_bytes: usize, observed: usize },
    PositionalArgsNotAllowed { function: String, count: usize },
    NonStringKeyword { function: String },
    UnsupportedInput { detail: String },
}

impl std::fmt::Display for ValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DepthExceeded { max_depth } => {
                write!(f, "value nesting depth exceeded limit of {max_depth}")
            }
            Self::SizeExceeded {
                max_bytes,
                observed,
            } => write!(
                f,
                "value payload size exceeded limit ({observed} bytes > {max_bytes} bytes)"
            ),
            Self::PositionalArgsNotAllowed { function, count } => write!(
                f,
                "{function}() accepts keyword arguments only ({count} positional argument(s) given)"
            ),
            Self::NonStringKeyword { function } => {
                write!(f, "{function}() keyword names must be strings")
            }
            Self::UnsupportedInput { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for ValueError {}

impl ValueError {
    #[must_use]
    pub fn into_monty_exception(self, signature_hint: Option<&str>) -> MontyException {
        match self {
            Self::PositionalArgsNotAllowed { .. } | Self::NonStringKeyword { .. } => {
                let msg = signature_hint.map_or_else(
                    || self.to_string(),
                    |sig| format!("{self}; signature: {sig}"),
                );
                MontyException::new(ExcType::TypeError, Some(msg))
            }
            Self::DepthExceeded { .. } | Self::SizeExceeded { .. } => {
                MontyException::new(ExcType::ValueError, Some(self.to_string()))
            }
            Self::UnsupportedInput { detail } => {
                MontyException::new(ExcType::TypeError, Some(detail))
            }
        }
    }
}

/// Converts keyword-only `CallArgs` into a JSON object for `TcCodeBackend`.
pub fn kwargs_to_json_object(
    function: &str,
    args: &CallArgs,
) -> Result<Map<String, Value>, ValueError> {
    let pos_count = args.args().len();
    if pos_count > 0 {
        return Err(ValueError::PositionalArgsNotAllowed {
            function: function.to_owned(),
            count: pos_count,
        });
    }
    let mut used_bytes = 0usize;
    let mut map = Map::with_capacity(args.kwargs().len());
    for (k, v) in args.kwargs() {
        let Some(key) = k.as_str() else {
            return Err(ValueError::NonStringKeyword {
                function: function.to_owned(),
            });
        };
        charge_bytes(&mut used_bytes, key.len())?;
        let json_val = object_ref_to_json(v, 1, &mut used_bytes)?;
        map.insert(key.to_owned(), json_val);
    }
    Ok(map)
}

/// Converts a sandbox `MontyObject` result into a JSON `Value` with bounded depth and size.
pub fn monty_to_json(value: &MontyObject) -> Result<Value, ValueError> {
    let mut used_bytes = 0usize;
    object_ref_to_json(value.as_ref(), 0, &mut used_bytes)
}

fn object_ref_to_json(
    obj: ObjectRef<'_>,
    depth: usize,
    used_bytes: &mut usize,
) -> Result<Value, ValueError> {
    if depth > MAX_VALUE_DEPTH {
        return Err(ValueError::DepthExceeded {
            max_depth: MAX_VALUE_DEPTH,
        });
    }
    match obj.type_name() {
        "NoneType" => Ok(Value::Null),
        "bool" => Ok(Value::Bool(obj.as_bool().unwrap_or(false))),
        "int" => {
            if let Some(i) = obj.as_int() {
                charge_bytes(used_bytes, 8)?;
                Ok(Value::Number(Number::from(i)))
            } else {
                let repr = obj.py_repr();
                charge_bytes(used_bytes, repr.len())?;
                Ok(Value::String(repr))
            }
        }
        "float" => {
            let f = obj.as_float().unwrap_or(f64::NAN);
            charge_bytes(used_bytes, 8)?;
            Ok(Number::from_f64(f).map_or(Value::Null, Value::Number))
        }
        "str" => {
            let s = obj.as_str().unwrap_or_default();
            charge_bytes(used_bytes, s.len())?;
            Ok(Value::String(s.to_owned()))
        }
        _ => {
            if let Some(items) = obj.items() {
                charge_bytes(used_bytes, items.len().saturating_mul(8))?;
                let mut arr = Vec::with_capacity(items.len());
                for item in items {
                    arr.push(object_ref_to_json(item, depth + 1, used_bytes)?);
                }
                Ok(Value::Array(arr))
            } else if let Some(pairs) = obj.pairs() {
                charge_bytes(used_bytes, pairs.len().saturating_mul(16))?;
                let mut map = Map::with_capacity(pairs.len());
                for (k, v) in pairs {
                    let key_str = k.as_str().map_or_else(|| k.py_repr(), str::to_owned);
                    charge_bytes(used_bytes, key_str.len())?;
                    let val = object_ref_to_json(v, depth + 1, used_bytes)?;
                    map.insert(key_str, val);
                }
                Ok(Value::Object(map))
            } else {
                let repr = obj.py_repr();
                charge_bytes(used_bytes, repr.len())?;
                Ok(Value::String(repr))
            }
        }
    }
}

/// Converts a `serde_json::Value` from a host call into a `MontyObject`.
pub fn json_to_monty(value: &Value) -> Result<MontyObject, ValueError> {
    let mut used_bytes = 0usize;
    json_to_monty_inner(value, 0, &mut used_bytes)
}

#[allow(clippy::cast_precision_loss)]
fn json_to_monty_inner(
    value: &Value,
    depth: usize,
    used_bytes: &mut usize,
) -> Result<MontyObject, ValueError> {
    if depth > MAX_VALUE_DEPTH {
        return Err(ValueError::DepthExceeded {
            max_depth: MAX_VALUE_DEPTH,
        });
    }
    match value {
        Value::Null => Ok(MontyObject::none()),
        Value::Bool(b) => Ok(MontyObject::bool(*b)),
        Value::Number(n) => {
            charge_bytes(used_bytes, 8)?;
            match (n.as_i64(), n.as_u64()) {
                (Some(i), _) => Ok(MontyObject::int(i)),
                (None, Some(u)) => Ok(i64::try_from(u)
                    .map_or_else(|_| MontyObject::float(u as f64), MontyObject::int)),
                (None, None) => Ok(MontyObject::float(n.as_f64().unwrap_or(0.0))),
            }
        }
        Value::String(s) => {
            charge_bytes(used_bytes, s.len())?;
            Ok(MontyObject::string(s.clone()))
        }
        Value::Array(items) => {
            charge_bytes(used_bytes, items.len().saturating_mul(8))?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_monty_inner(item, depth + 1, used_bytes)?);
            }
            Ok(MontyObject::list(out))
        }
        Value::Object(map) => {
            charge_bytes(used_bytes, map.len().saturating_mul(16))?;
            let mut pairs = Vec::with_capacity(map.len());
            for (k, v) in map {
                charge_bytes(used_bytes, k.len())?;
                let mv = json_to_monty_inner(v, depth + 1, used_bytes)?;
                pairs.push((MontyObject::string(k.clone()), mv));
            }
            Ok(MontyObject::dict(pairs))
        }
    }
}

const fn charge_bytes(used: &mut usize, add: usize) -> Result<(), ValueError> {
    let next = used.saturating_add(add);
    if next > MAX_VALUE_BYTES {
        return Err(ValueError::SizeExceeded {
            max_bytes: MAX_VALUE_BYTES,
            observed: next,
        });
    }
    *used = next;
    Ok(())
}

/// Drops a potentially deeply nested `serde_json::Value` iteratively on the heap
/// so maliciously deep JSON trees cannot blow the worker thread call stack on Drop.
pub fn drop_json_iteratively(root: Value) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node {
            Value::Array(mut items) => {
                stack.append(&mut items);
            }
            Value::Object(map) => {
                stack.extend(map.into_values());
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
}
