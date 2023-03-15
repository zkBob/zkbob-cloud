use serde::{Serialize, Deserialize};

use crate::account::history::{HistoryTxType, HistoryTx};


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