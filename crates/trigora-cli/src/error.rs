#[derive(Debug)]
pub struct CliError {
    pub title: String,
    pub message: Option<String>,
    pub details: Vec<(String, String)>,
    pub hint: Option<String>,
    pub code: i32,
    pub raw: bool,
}

impl CliError {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: None,
            details: Vec::new(),
            hint: None,
            code: 1,
            raw: false,
        }
    }

    pub fn plain(reason: impl Into<String>) -> Self {
        Self::new("Command failed").detail("Reason", reason)
    }

    /// A usage error already formatted by the argument parser. Print it unchanged.
    pub fn usage(text: impl Into<String>) -> Self {
        Self {
            title: String::new(),
            message: Some(text.into()),
            details: Vec::new(),
            hint: None,
            code: 2,
            raw: true,
        }
    }

    pub fn detail(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((label.into(), value.into()));
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        let message = message.into();
        self.message = Some(message);
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.title)
    }
}
