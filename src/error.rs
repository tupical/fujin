//! Local error type.
//!
//! Replaces `taskagent_shared::CoreError` so the skeleton is
//! dependency-free. mcpbox maps [`ActionsError`] onto its own error surface
//! when wiring the layer.

use std::fmt;

#[derive(Debug)]
pub enum ActionsError {
    /// Output failed validation (missing or invalid fields).
    Validation(String),
    /// (De)serialization failure.
    Serde(String),
}

impl ActionsError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }
    pub fn serde(msg: impl Into<String>) -> Self {
        Self::Serde(msg.into())
    }
}

impl fmt::Display for ActionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(m) => write!(f, "validation: {m}"),
            Self::Serde(m) => write!(f, "serde: {m}"),
        }
    }
}

impl std::error::Error for ActionsError {}
