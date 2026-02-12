use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum CoreError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Invalid event type: {0}")]
    InvalidEventType(String),

    #[error("Invalid quality level: {0}")]
    InvalidQuality(String),

    #[error("Credit score calculation error: {0}")]
    CreditCalculationError(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
