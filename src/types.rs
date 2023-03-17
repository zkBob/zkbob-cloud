use serde::{Serialize, Deserialize};

use crate::{account::history::{HistoryTxType, HistoryTx}, cloud::types::{TransferPart, TransferStatus}};


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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusRequest {
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
pub struct ExportKeyResponse {
    pub sk: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_tx_hashes: Option<Vec<String>>,
    pub timestamp: u64,
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}

impl HistoryRecord {
    pub fn prepare_records(txs: Vec<HistoryTx>) -> Vec<HistoryRecord> {
        txs.iter()
            .filter(|tx| tx.tx_type != HistoryTxType::AggregateNotes)
            .map(|tx| {
                let fee = (tx.tx_type != HistoryTxType::TransferIn && tx.tx_type != HistoryTxType::DirectDeposit).then(|| tx.fee);

                match tx.transaction_id.clone() {
                    Some(request_id) => {
                        let linked_txs = txs
                            .iter()
                            .filter(|tx| tx.transaction_id == Some(request_id.clone()))
                            .filter(|tx| tx.tx_type == HistoryTxType::AggregateNotes);
                        
                        let linked_tx_hashes = linked_txs
                            .clone()
                            .map(|linked_tx| linked_tx.tx_hash.clone())
                            .collect::<Vec<_>>();

                        let linked_tx_hashes = (!linked_tx_hashes.is_empty()).then(|| linked_tx_hashes);

                        let fee = fee.map(|fee| fee + linked_txs.map(|tx| tx.fee).sum::<u64>());

                        HistoryRecord {
                            tx_type: tx.tx_type.clone(),
                            tx_hash: tx.tx_hash.clone(),
                            linked_tx_hashes,
                            fee,
                            timestamp: tx.timestamp.clone(),
                            amount: tx.amount.clone(),
                            to: tx.to.clone(),
                            transaction_id: Some(request_id),
                        }
                    },
                    None => HistoryRecord {
                        tx_type: tx.tx_type.clone(),
                        tx_hash: tx.tx_hash.clone(),
                        linked_tx_hashes: None,
                        fee,
                        timestamp: tx.timestamp.clone(),
                        amount: tx.amount.clone(),
                        to: tx.to.clone(),
                        transaction_id: None,
                    },
                }
            })
            .collect::<Vec<_>>()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusResponse {
    pub status: String,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_tx_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

impl TransactionStatusResponse {
    pub fn from(parts: Vec<TransferPart>) -> Self {
        let mut tx_hashes = parts
            .iter()
            .filter(|job| job.tx_hash.is_some() && job.status != TransferStatus::Mining)
            .map(|job| job.tx_hash.clone().unwrap())
            .collect::<Vec<_>>();

        let tx_hash = tx_hashes.pop();
        let linked_tx_hashes = tx_hash.is_some().then(|| tx_hashes);

        let (status, timestamp, failure_reason) = {
            let last = parts.last().unwrap();
            match last.status {
                TransferStatus::Done => (TransferStatus::Done.status(), last.timestamp, None),
                TransferStatus::Failed(_) => {
                    let first_failed_part = parts
                        .iter()
                        .filter(|job| match job.status {
                            TransferStatus::Failed(_) => true,
                            _ => false,
                        })
                        .collect::<Vec<_>>()
                        .first()
                        .unwrap()
                        .clone();

                    (
                        first_failed_part.status.status(),
                        first_failed_part.timestamp,
                        first_failed_part.status.failure_reason(),
                    )
                }
                _ => {
                    let relevant_part = parts
                        .iter()
                        .filter(|job| job.status != TransferStatus::New)
                        .last();
                    match relevant_part {
                        Some(relevant_part) => (TransferStatus::Relaying.status(), relevant_part.timestamp, None),
                        None => (TransferStatus::New.status(), parts[0].timestamp, None)
                    }
                }
            }
        };

        TransactionStatusResponse {
            status,
            timestamp,
            tx_hash,
            linked_tx_hashes,
            failure_reason,
        }
    }
}