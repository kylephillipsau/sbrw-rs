//! The JavaScript in soapbox-race-core's game data (achievement triggers and
//! values, reward scripts), run as its `ScriptingBO` ran it on Nashorn, here
//! on QuickJS.
//!
//! A script sees its bindings as Nashorn showed it Java objects:
//! - a bean's property `foo` both as `x.foo` and as `x.getFoo()` (`x.isFoo()`
//!   for a `boolean`), and an unknown property as `undefined`;
//! - `null` as `null`, a `Boolean` as a boolean, a `String` or an enum as a
//!   string, and any other object (a date) as a non-null object;
//! - an `Integer`, `Float` or `Double` as a number.
//!
//! A `Long` is a number too. Nashorn keeps it a `java.lang.Long` that
//! compares and computes as a number (`fixtures/vectors/nashorn.json`), so
//! the scripts in the data behave the same either way. Only these differ, and
//! no data script uses them: `typeof` (`object` in Nashorn), `===` against a
//! number, and values past 2^53.
//!
//! Each evaluation gets fresh globals, as `eval(script, new SimpleBindings(..))`
//! did. Unlike Nashorn, a script is stopped after [`TIME_LIMIT`] and limited
//! in memory, so a bad edit to the data can't hang the server.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Function, Object, Runtime, Type, Value};

/// How long one evaluation may run.
pub const TIME_LIMIT: Duration = Duration::from_secs(1);
const MEMORY_LIMIT: usize = 32 << 20;

/// A Java value as a script sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum JavaValue {
    Null,
    Boolean(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    /// An enum constant, which scripts compare to strings by name.
    Enum(String),
    /// Any other object: not null, and its `toString()`.
    Object(String),
    Bean(Bean),
}

/// A JavaBean: the properties its getters expose.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bean {
    properties: Vec<Property>,
}

#[derive(Debug, Clone, PartialEq)]
struct Property {
    name: &'static str,
    getter: String,
    value: JavaValue,
}

impl Bean {
    pub fn new() -> Self {
        Self::default()
    }

    /// A property read by `getName()`.
    pub fn get(self, name: &'static str, value: JavaValue) -> Self {
        self.property("get", name, value)
    }

    /// A `boolean` property read by `isName()`.
    pub fn is(self, name: &'static str, value: bool) -> Self {
        self.property("is", name, JavaValue::Boolean(value))
    }

    fn property(mut self, prefix: &str, name: &'static str, value: JavaValue) -> Self {
        let mut chars = name.chars();
        let getter = match chars.next() {
            Some(first) => format!("{prefix}{}{}", first.to_ascii_uppercase(), chars.as_str()),
            None => prefix.to_owned(),
        };
        self.properties.push(Property { name, getter, value });
        self
    }
}

/// What a script evaluated to, as the Java object Nashorn returned. Numbers
/// are an `Integer` when QuickJS holds them as one, otherwise a `Double`;
/// Nashorn picks between `Integer` and `Double` differently, but upstream
/// only ever uses a number's value.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptValue {
    Null,
    Boolean(bool),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    /// Any other object, by its string form.
    Object(String),
}

/// `javax.script.ScriptException`: the script failed to compile, threw, or
/// was stopped.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptException(pub String);

impl std::fmt::Display for ScriptException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScriptException {}

thread_local! {
    /// One runtime per thread; QuickJS runtimes can't be shared between threads.
    static RUNTIME: RefCell<Option<(Runtime, std::rc::Rc<std::cell::Cell<Instant>>)>> = const { RefCell::new(None) };
}

/// Evaluate `script` with these globals.
pub fn eval(script: &str, bindings: &[(&str, JavaValue)]) -> Result<ScriptValue, ScriptException> {
    RUNTIME.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(new_runtime().map_err(|e| ScriptException(format!("cannot start the script engine: {e}")))?);
        }
        let (runtime, deadline) = slot.as_ref().expect("set above");
        deadline.set(Instant::now() + TIME_LIMIT);
        let context = Context::full(runtime).map_err(|e| ScriptException(e.to_string()))?;
        context.with(|ctx| run(&ctx, script, bindings))
    })
}

fn new_runtime() -> rquickjs::Result<(Runtime, std::rc::Rc<std::cell::Cell<Instant>>)> {
    let runtime = Runtime::new()?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    let deadline = std::rc::Rc::new(std::cell::Cell::new(Instant::now()));
    let handler_deadline = deadline.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > handler_deadline.get())));
    Ok((runtime, deadline))
}

fn run(ctx: &Ctx<'_>, script: &str, bindings: &[(&str, JavaValue)]) -> Result<ScriptValue, ScriptException> {
    let globals = ctx.globals();
    let getter: Function =
        ctx.eval("(function (v) { return function () { return v; }; })").catch(ctx).map_err(|e| exception(&e))?;
    for (name, value) in bindings {
        let value = to_js(ctx, &getter, value).catch(ctx).map_err(|e| exception(&e))?;
        globals.set(*name, value).catch(ctx).map_err(|e| exception(&e))?;
    }
    let result: Value = ctx.eval(script).catch(ctx).map_err(|e| exception(&e))?;
    from_js(ctx, &result).catch(ctx).map_err(|e| exception(&e))
}

fn exception(error: &CaughtError<'_>) -> ScriptException {
    ScriptException(match error {
        CaughtError::Exception(e) => e.message().unwrap_or_else(|| e.to_string()),
        other => other.to_string(),
    })
}

/// `getter` makes a getter method that returns its argument.
fn to_js<'js>(ctx: &Ctx<'js>, getter: &Function<'js>, value: &JavaValue) -> rquickjs::Result<Value<'js>> {
    Ok(match value {
        JavaValue::Null => Value::new_null(ctx.clone()),
        JavaValue::Boolean(b) => Value::new_bool(ctx.clone(), *b),
        JavaValue::Integer(i) => Value::new_int(ctx.clone(), *i),
        JavaValue::Long(l) => Value::new_number(ctx.clone(), *l as f64),
        JavaValue::Float(f) => Value::new_number(ctx.clone(), f64::from(*f)),
        JavaValue::Double(d) => Value::new_number(ctx.clone(), *d),
        JavaValue::String(s) | JavaValue::Enum(s) => rquickjs::String::from_str(ctx.clone(), s)?.into_value(),
        JavaValue::Object(display) => {
            let object = Object::new(ctx.clone())?;
            let display = display.clone();
            object.set("toString", Function::new(ctx.clone(), move || display.clone())?)?;
            object.into_value()
        }
        JavaValue::Bean(bean) => {
            let object = Object::new(ctx.clone())?;
            for property in &bean.properties {
                let value = to_js(ctx, getter, &property.value)?;
                object.set(property.name, value.clone())?;
                object.set(property.getter.as_str(), getter.call::<_, Value>((value,))?)?;
            }
            object.into_value()
        }
    })
}

fn from_js<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> rquickjs::Result<ScriptValue> {
    Ok(match value.type_of() {
        Type::Uninitialized | Type::Undefined | Type::Null => ScriptValue::Null,
        Type::Bool => ScriptValue::Boolean(value.as_bool().unwrap_or_default()),
        Type::Int => ScriptValue::Integer(value.as_int().unwrap_or_default()),
        Type::Float => ScriptValue::Double(value.as_float().unwrap_or_default()),
        Type::String => ScriptValue::String(value.as_string().map(|s| s.to_string()).transpose()?.unwrap_or_default()),
        _ => {
            let string: Function = ctx.globals().get("String")?;
            ScriptValue::Object(string.call((value.clone(),))?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(script: &str) -> Result<ScriptValue, ScriptException> {
        let car = Bean::new()
            .get("id", JavaValue::Long(1))
            .get("expirationDate", JavaValue::Null)
            .is("preset", true)
            .get("name", JavaValue::String("r32".into()));
        eval(script, &[("car", JavaValue::Bean(car)), ("l5", JavaValue::Long(5)), ("n", JavaValue::Null)])
    }

    #[test]
    fn beans_have_properties_and_getters() {
        assert_eq!(run("car.getExpirationDate() == null"), Ok(ScriptValue::Boolean(true)));
        assert_eq!(run("car.expirationDate == null"), Ok(ScriptValue::Boolean(true)));
        assert_eq!(run("car.isPreset() && car.preset"), Ok(ScriptValue::Boolean(true)));
        assert_eq!(run("car.name == 'r32'"), Ok(ScriptValue::Boolean(true)));
        assert_eq!(run("car.nothing"), Ok(ScriptValue::Null));
        assert!(run("car.getNothing()").is_err());
        assert!(run("n.foo").is_err());
        assert!(run("nothing").is_err());
    }

    #[test]
    fn longs_compute_as_numbers() {
        assert_eq!(run("l5 == 5"), Ok(ScriptValue::Boolean(true)));
        assert_eq!(run("1"), Ok(ScriptValue::Integer(1)));
        assert_eq!(run("1609.34 * 2"), Ok(ScriptValue::Double(3218.68)));
        assert!(matches!(run("(l5 + 1) * +(car.preset)"), Ok(ScriptValue::Integer(6) | ScriptValue::Double(6.0))));
    }

    #[test]
    fn each_evaluation_has_fresh_globals() {
        assert_eq!(run("var leaked = 1; leaked"), Ok(ScriptValue::Integer(1)));
        assert!(run("leaked").is_err());
    }

    #[test]
    fn runaway_scripts_are_stopped() {
        let started = Instant::now();
        assert!(run("while (true) {}").is_err());
        assert!(started.elapsed() < TIME_LIMIT * 3);
        assert_eq!(run("1"), Ok(ScriptValue::Integer(1)), "the engine still works afterwards");
    }
}
