use thiserror::Error;

#[derive(Error, Debug)]
#[error("Request was blocked")]
pub struct BlockedRequestError;

#[derive(Error, Debug)]
#[error("Request reached limit block. Check the proxies or try again later")]
pub struct BlockedRequestLimitError;

#[derive(Error, Debug)]
#[error("Invalid form action URL {0}")]
pub struct InvalidFormActionError(pub String);

#[derive(Error, Debug)]
#[error("No connection")]
pub struct NoConnectionError;

#[derive(Error, Debug)]
#[error("No VPN set")]
pub struct VPNNotSetError;

#[derive(Error, Debug)]
#[error("Unsupported Browser")]
pub struct UnsupportedBrowserError(pub String);

#[derive(Error, Debug)]
#[error("Max tries exceeded")]
pub struct MaxTriesExceededError;

#[derive(Error, Debug)]
#[error("Mobile number not allowed")]
pub struct MobileNumberNotAllowedError;

#[derive(Error, Debug)]
#[error("Invalid phone number")]
pub struct InvalidPhoneNumberError;

#[derive(Error, Debug)]
#[error("No payload")]
pub struct NoPayloadError;

#[derive(Error, Debug)]
#[error("Could not download file")]
pub struct DownloadError;

#[derive(Error, Debug)]
#[error("Could not resolve captcha audio")]
pub struct UnresolvedCaptchaAudioError;
