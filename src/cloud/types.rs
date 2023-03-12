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
    Queued,
    Proving,
    Relaying,
    Mining,
    Done,
    Failed(CloudError),
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
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TransferTask {
    pub request_id: String,
    pub parts: Vec<String>
}