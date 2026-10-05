//! Upstream's error model: `EngineException` and how its container treated exceptions.

mod codes;

pub use codes::EngineExceptionCode;

/// `com.soapboxrace.core.engine.EngineException`. An application exception:
/// work done before it is thrown still commits, and the client gets a 503 with
/// the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineException {
    pub code: EngineExceptionCode,
    pub fatal: bool,
    pub message: Option<String>,
}

impl EngineException {
    pub fn new(code: EngineExceptionCode, fatal: bool) -> Self {
        Self { code, fatal, message: None }
    }
}

/// What a world call can fail with, by how upstream's container handled it.
#[derive(Debug, thiserror::Error)]
pub enum Fault {
    /// `EngineException`: commits, then 503 with the error code.
    #[error("{0:?}")]
    Engine(EngineException),
    /// `javax.ws.rs.NotAuthorizedException`: rolls back, then 401 with no body.
    #[error("not authorized: {0}")]
    NotAuthorized(String),
    /// Any other exception: rolls back, then 503 with `UnspecifiedError` (-500).
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl Fault {
    pub fn engine(code: EngineExceptionCode, fatal: bool) -> Self {
        Self::Engine(EngineException::new(code, fatal))
    }

    /// A call into upstream code that isn't ported yet (`unit` is its id):
    /// fails like a system exception, so the transaction rolls back.
    pub fn unported(unit: &str) -> Self {
        Self::Internal(anyhow::anyhow!("not ported yet: {unit}"))
    }

    /// Where upstream dereferences a null (`what`) and throws a
    /// `NullPointerException`.
    pub fn null_pointer(what: &str) -> Self {
        Self::Internal(anyhow::anyhow!("NullPointerException: {what} is null"))
    }

    /// Whether the transaction commits despite the failure.
    pub fn commits(&self) -> bool {
        matches!(self, Self::Engine(_))
    }
}

impl From<rusqlite::Error> for Fault {
    fn from(e: rusqlite::Error) -> Self {
        Self::Internal(e.into())
    }
}
