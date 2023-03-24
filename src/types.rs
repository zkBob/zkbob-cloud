use serde::{Deserialize, Serialize};

use crate::{
    account::history::HistoryTxType,
    cloud::types::{TransferPart, TransferStatus, ReportStatus, Report, CloudHistoryTx},
};

#[derive(Serialize, Deserialize)]
pub struct SignupRequest {
    pub id: Option<String>,
    pub description: String,
    pub sk: Option<String>,
}

#[derive(Deserialize)]
pub struct ImportRequestItem {
    pub id: String,
    pub description: String,
    pub sk: String,
}

pub type ImportRequest = Vec<ImportRequestItem>;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupResponse {
    pub account_id: String,
}

#[derive(Deserialize)]
pub struct AccountInfoRequest {
    pub id: String,
}

#[derive(Deserialize)]
pub struct ReportRequest {
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct ReportResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ReportStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Report>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAddressResponse {
    pub address: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub transaction_id: Option<String>,
    pub account_id: String,
    pub amount: u64,
    pub to: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferResponse {
    pub transaction_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionStatusRequest {
    pub transaction_id: String,
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
    pub fn prepare_records(txs: Vec<CloudHistoryTx>) -> Vec<HistoryRecord> {
        txs.iter()
            .filter(|tx| tx.tx_type != HistoryTxType::AggregateNotes)
            .map(|tx| {
                let fee = (tx.tx_type != HistoryTxType::TransferIn
                    && tx.tx_type != HistoryTxType::DirectDeposit)
                    .then_some(tx.fee);

                match tx.transaction_id.clone() {
                    Some(transaction_id) => {
                        let linked_txs = txs
                            .iter()
                            .filter(|tx| tx.transaction_id.as_ref() == Some(&transaction_id))
                            .filter(|tx| tx.tx_type == HistoryTxType::AggregateNotes);

                        let linked_tx_hashes = linked_txs
                            .clone()
                            .map(|linked_tx| linked_tx.tx_hash.clone())
                            .collect::<Vec<_>>();

                        let linked_tx_hashes =
                            (!linked_tx_hashes.is_empty()).then_some(linked_tx_hashes);

                        let fee = fee.map(|fee| fee + linked_txs.map(|tx| tx.fee).sum::<u64>());

                        HistoryRecord {
                            tx_type: tx.tx_type.clone(),
                            tx_hash: tx.tx_hash.clone(),
                            linked_tx_hashes,
                            fee,
                            timestamp: tx.timestamp,
                            amount: tx.amount,
                            to: tx.to.clone(),
                            transaction_id: Some(transaction_id),
                        }
                    }
                    None => HistoryRecord {
                        tx_type: tx.tx_type.clone(),
                        tx_hash: tx.tx_hash.clone(),
                        linked_tx_hashes: None,
                        fee,
                        timestamp: tx.timestamp,
                        amount: tx.amount,
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
            .filter_map(|part| match &part.tx_hash {
                Some(tx_hash) if part.status != TransferStatus::Mining => Some(tx_hash.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        let tx_hash = tx_hashes.pop();
        let linked_tx_hashes = tx_hash.is_some().then_some(tx_hashes);

        let (status, timestamp, failure_reason) = {
            let last = parts.last().unwrap();
            match last.status {
                TransferStatus::Done => (TransferStatus::Done.status(), last.timestamp, None),
                TransferStatus::Failed(_) => {
                    let first_failed_part = &(*parts
                        .iter()
                        .find(|job| matches!(job.status, TransferStatus::Failed(_)))
                        .unwrap())
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
                        Some(relevant_part) => (
                            TransferStatus::Relaying.status(),
                            relevant_part.timestamp,
                            None,
                        ),
                        None => (TransferStatus::New.status(), parts[0].timestamp, None),
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
