//! `ScriptingBO`: the JavaScript in upstream's game data (achievement triggers
//! and values, reward scripts), which upstream ran on Nashorn. Not ported yet:
//! a JavaScript engine replaces Nashorn with the first slice whose data has
//! scripts to run. Until then, evaluating anything fails like an unported call.

use crate::Fault;
use crate::bo::achievement_transaction::Bindings;

/// The Java value a script evaluated to, as Nashorn handed it back.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptValue {
    Null,
    Boolean(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
}

/// `javax.script.ScriptException`: the script failed to compile or threw.
/// Callers catch it, unlike any other failure.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptException(pub String);

/// `ScriptingBO.eval(script, bindings)`.
pub fn eval(_script: &str, _bindings: &Bindings) -> Result<Result<ScriptValue, ScriptException>, Fault> {
    Err(Fault::unported("bo/ScriptingBO#eval(String,Map)"))
}
