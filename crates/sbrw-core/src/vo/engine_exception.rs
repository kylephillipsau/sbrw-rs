//! `EngineExceptionTrans`: the body of every 503 the game receives.

use sbrw_xml::{ToXml, XmlWriter};

#[derive(Debug, Clone, PartialEq)]
pub struct EngineExceptionTrans {
    pub error_code: i32,
    /// Upstream sends a Java stack trace here; replays don't compare it.
    pub stack_trace: String,
}

impl ToXml for EngineExceptionTrans {
    fn to_xml(&self) -> String {
        let mut w = XmlWriter::new();
        w.open("EngineExceptionTrans");
        w.value("ErrorCode", self.error_code);
        w.open("InnerException");
        w.value("ErrorCode", self.error_code);
        w.text("StackTrace", &self.stack_trace);
        w.close();
        w.text("StackTrace", &self.stack_trace);
        w.close();
        w.finish()
    }
}
