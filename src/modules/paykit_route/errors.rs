use thiserror::Error;

#[derive(uniffi::Error, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaykitRouteError {
    #[error("Invalid route request: {message}")]
    InvalidRequest { message: String },
}
