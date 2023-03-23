use libzkbob_rs::libzeropool::fawkes_crypto::ff_uint::Num;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::{Fr, errors::CloudError};


#[derive(Serialize, Deserialize, Debug)]
pub struct AccountData {
    pub description: String,
    pub db_path: String,
    pub sk: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub sk: String,
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
        matches!(self, TransferStatus::Done | TransferStatus::Failed(_))
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
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TransferTask {
    pub request_id: String,
    pub parts: Vec<String>
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountReport {
    pub id: String,
    pub description: String,
    pub balance: u64,
    pub max_transfer_amount: u64,
    pub address: String,
    pub sk: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub timestamp: u64,
    pub pool_index: u64,
    pub account: Vec<AccountReport>
}


#[derive(Serialize, Deserialize, Debug)]
pub enum ReportStatus {
    New,
    Completed,
    Failed,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReportTask {
    pub status: ReportStatus,
    pub attempt: u32,
    pub report: Option<Report>,
}