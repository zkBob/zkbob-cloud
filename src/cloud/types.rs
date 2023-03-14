use libzkbob_rs::libzeropool::fawkes_crypto::ff_uint::Num;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::{Fr, errors::CloudError};

#[derive(Serialize, Deserialize)]
pub struct SignupRequest {
    pub id: Option<String>,
    pub sk: Option<String>,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupResponse {
    pub account_id: String,
}

#[derive(Deserialize)]
pub struct AccountInfoRequest {
    pub id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAddressResponse {
    pub address: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub request_id: Option<String>,
    pub account_id: String,
    pub amount: u64,
    pub to: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResponse {
    pub request_id: String,
}

pub struct Transfer {
    pub id: String,
    pub account_id: Uuid,
    pub amount: u64,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TransferStatus {
    New,
    Proving,
    Relaying,
    Mining,
    Done,
    Failed(CloudError),
}

impl TransferStatus {
    pub fn from_relayer_response(status: String, failure_reason: Option<String>) -> Self {
        match status.as_str() {
            "waiting" => Self::Relaying,
            "sent" => Self::Mining,
            "completed" => Self::Done,
            "reverted" => Self::Failed(CloudError::TaskRejectedByRelayer(
                failure_reason.unwrap_or(Default::default()),
            )),
            "failed" => Self::Failed(CloudError::TaskRejectedByRelayer(
                failure_reason.unwrap_or(Default::default()),
            )),
            _ => Self::Failed(CloudError::RelayerSendError),
        }
    }

    pub fn is_final(&self) -> bool {
        match self {
            TransferStatus::Done | TransferStatus::Failed(_) => true,
            _ => false,
        }
    }

    pub fn status(&self) -> String {
        match self {
            Self::Failed(_) => "Failed".to_string(),
            _ => format!("{:?}", self),
        }
    }

    pub fn failure_reason(&self) -> Option<String> {
        match self {
            Self::Failed(err) => Some(err.to_string()),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransferPart {
    pub id: String,
    pub request_id: String,
    pub account_id: String,
    pub amount: Num<Fr>,
    pub fee: u64,
    pub to: Option<String>,
    pub status: TransferStatus,
    pub job_id: Option<String>,
    pub tx_hash: Option<String>,
    pub depends_on: Option<String>,
    pub attempt: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TransferTask {
    pub request_id: String,
    pub parts: Vec<String>
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusRequest {
    pub request_id: String,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CalculateFeeRequest {
    pub account_id: String,
    pub amount: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculateFeeResponse {
    pub transaction_count: u64,
    pub total_fee: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
}