use thiserror::Error;

#[derive(uniffi::Error, Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaykitRouteError {
    #[error("Invalid route request: {error_details}")]
    InvalidRequest { error_details: String },
}
