use std::fmt;

#[derive(Debug)]
pub struct LocalError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl LocalError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self::new(500, "invalid_input", message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_input", message)
    }

    pub fn body(self) -> serde_json::Value {
        serde_json::json!({
            "error": { "code": self.code, "message": self.message }
        })
    }
}

impl fmt::Display for LocalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LocalError {}

impl From<rusqlite::Error> for LocalError {
    fn from(err: rusqlite::Error) -> Self {
        Self::message(err.to_string())
    }
}

impl From<tcc_host::HostError> for LocalError {
    fn from(err: tcc_host::HostError) -> Self {
        Self::message(err.to_string())
    }
}

impl From<tcc_state::StateError> for LocalError {
    fn from(err: tcc_state::StateError) -> Self {
        Self::message(err.to_string())
    }
}
