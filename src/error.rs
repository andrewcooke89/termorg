use std::fmt;

/// Errors visible at the CLI boundary — always human-readable.
#[derive(Debug)]
pub enum TermorgError {
    ProviderUnavailable { provider: String, message: String },
    ProviderCommand { message: String },
    Parse { message: String },
    Io(std::io::Error),
}

impl fmt::Display for TermorgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderUnavailable { provider, message } => {
                write!(
                    f,
                    "Cannot see terminals from provider '{provider}': {message}"
                )
            }
            Self::ProviderCommand { message } => {
                write!(f, "Terminal provider command failed: {message}")
            }
            Self::Parse { message } => write!(f, "Could not parse provider response: {message}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for TermorgError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TermorgError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, TermorgError>;
