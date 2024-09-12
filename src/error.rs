use thiserror::Error;

#[derive(Error, Debug)]
#[error("Request was blocked")]
pub struct BlockedRequestError;

#[derive(Error, Debug)]
#[error("No connection")]
pub struct NoConnectionError;

#[derive(Error, Debug)]
#[error("Unsupported Browser")]
pub struct UnsupportedBrowserError(pub String);

#[derive(Error, Debug)]
#[error("Max tries exceeded")]
pub struct MaxTriesExceededError;

#[derive(Error, Debug)]
#[error("Error parsing enum of type {0}")]
pub struct ParseEnumError(pub String);

#[derive(Error, Debug)]
#[error("Rate limit timeout exceeded")]
pub struct RateLimitTimeoutExceededError;

#[derive(Error, Debug)]
#[error("Error parsing arguments. {0}")]
pub struct ParseArgsError(pub String);

#[derive(Error, Debug)]
#[error("Invalid email address")]
pub struct InvalidEmailError;

#[derive(Error, Debug)]
#[error("Application exited with error {0}")]
pub struct ExitCodeError(pub i32);
