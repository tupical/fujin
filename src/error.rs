//! Local error type.
//!
//! Replaces `daruma_shared::CoreError` so the skeleton is
//! dependency-free. the host maps [`ActionsError`] onto its own error surface
//! when wiring the layer.

/// Local error type. Hand-expanded `layer_kit::layer_error!` shape plus
/// `NeedsInput`: the layer cannot proceed without a human answer, which the
/// host must surface as a question, not as a failure.
#[derive(Debug)]
pub enum ActionsError {
    /// AI provider failed or returned an unusable response.
    Ai(String),
    /// Output failed validation (missing or invalid fields).
    Validation(String),
    /// (De)serialization failure.
    Serde(String),
    /// The packet cannot be completed without information only a human has.
    NeedsInput(Vec<String>),
}

impl ActionsError {
    pub fn ai(msg: impl Into<String>) -> Self {
        Self::Ai(msg.into())
    }
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
    pub fn serde(msg: impl Into<String>) -> Self {
        Self::Serde(msg.into())
    }
}

impl std::fmt::Display for ActionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ai(m) => write!(f, "ai: {m}"),
            Self::Validation(m) => write!(f, "validation: {m}"),
            Self::Serde(m) => write!(f, "serde: {m}"),
            Self::NeedsInput(q) => write!(f, "needs_input: {}", q.join("; ")),
        }
    }
}

impl std::error::Error for ActionsError {}

impl From<crate::ai::AiError> for ActionsError {
    fn from(value: crate::ai::AiError) -> Self {
        Self::ai(value.to_string())
    }
}
