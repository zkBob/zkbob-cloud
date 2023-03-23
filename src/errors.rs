use actix_http::StatusCode;
use actix_web::{http::header::ContentType, HttpResponse, ResponseError};
use hex::FromHexError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Serialize, Deserialize, Debug, Error, PartialEq)]
pub enum CloudError {
    #[error("request malformed or invalid: {0}")]
    BadRequest(String),
    #[error("internal error")]
    CustodyLockError,
    #[error("internal error")]
    StateSyncError,
    #[error("bad account id")]
    IncorrectAccountId,
    #[error("bad account id")]
    AccountNotFound,
    #[error("duplicate account id")]
    DuplicateAccountId,
    #[error("request id cannot contain '.'")]
    InvalidTransactionId,
    #[error("request id already exists")]
    DuplicateTransactionId,
    #[error("internal error")]
    DataBaseReadError(String),
    #[error("internal error")]
    DataBaseWriteError(String),
    #[error("internal error")]
    RelayerSendError,
    #[error("request not found")]
    TransactionNotFound,
    #[error("general error occured:'{0}'")]
    InternalError(String),
    #[error("retries exhausted")]
    RetriesExhausted,
    #[error("relayer returned error: '{0}'")]
    TaskRejectedByRelayer(String),
    #[error("need retry")]
    RetryNeeded,
    #[error("access denied")]
    AccessDenied,
    #[error("previous tx failed")]
    PreviousTxFailed,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("account is busy")]
    AccountIsBusy,
    #[error("account is not synced yet")]
    AccountIsNotSynced,
    #[error("service is busy")]
    ServiceIsBusy,
    #[error("transaction expired")]
    TransactionExpired,
    #[error("transaction status is unknown")]
    TransactionStatusUnknown,
    #[error("failed to parse config")]
    ConfigError(String),
    #[error("rpc error")]
    Web3Error,
    #[error("bad report id")]
    ReportNotFound,
}

impl ResponseError for CloudError {
    fn status_code(&self) -> actix_http::StatusCode {
        match self {
            CloudError::TransactionNotFound
            | CloudError::DuplicateTransactionId
            | CloudError::BadRequest(_)
            | CloudError::IncorrectAccountId
            | CloudError::AccountNotFound => StatusCode::BAD_REQUEST,
            CloudError::AccessDenied => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        #[derive(Serialize)]
        struct ErrorResponse {
            error: String,
        }

        let response = serde_json::to_string(&ErrorResponse {
            error: format!("{}", self),
        })
        .unwrap_or(self.to_string());

        HttpResponse::build(self.status_code())
            .insert_header(ContentType::json())
            .body(response)
    }
}

impl From<config::ConfigError> for CloudError {
    fn from(e: config::ConfigError) -> Self {
        Self::ConfigError(e.to_string())
    }
}

impl From<zkbob_utils_rs::relayer::error::RelayerError> for CloudError {
    fn from(_: zkbob_utils_rs::relayer::error::RelayerError) -> Self {
        Self::RelayerSendError
    }
}

impl From<zkbob_utils_rs::contracts::error::PoolError> for CloudError {
    fn from(e: zkbob_utils_rs::contracts::error::PoolError) -> Self {
        Self::InternalError(e.to_string())
    }
}

impl From<FromHexError> for CloudError {
    fn from(e: FromHexError) -> Self {
        Self::InternalError(e.to_string())
    }
}