use serde_json::Error as JsonError;
use std::io::Error as IoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Fs(String),
    #[error("{0}")]
    Device(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    Transport(String),
    #[error("{0}")]
    TransferRestart(String),
    #[error("{message}")]
    Checksum { status: u8, message: String },
    #[error("{message}")]
    Remote {
        message: String,
        status: Option<u8>,
        os_error: Option<i32>,
    },
    #[error(transparent)]
    Io(#[from] IoError),
    #[error("{0}")]
    Json(String),
}

impl Error {
    pub fn remote(message: impl Into<String>, status: Option<u8>, os_error: Option<i32>) -> Self {
        Self::Remote {
            message: message.into(),
            status,
            os_error,
        }
    }

    pub fn os_error(&self) -> Option<i32> {
        match self {
            Self::Remote { os_error, .. } => *os_error,
            _ => None,
        }
    }
}

impl From<JsonError> for Error {
    fn from(error: JsonError) -> Self {
        Self::Json(error.to_string())
    }
}
