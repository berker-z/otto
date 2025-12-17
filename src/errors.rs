use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Auth expired for account")]
    AuthExpired,
    #[error("Config error: {0}")]
    Config(String),
    #[error("Unexpected error: {0}")]
    Unexpected(String),
}
