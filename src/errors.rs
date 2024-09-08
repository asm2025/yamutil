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
