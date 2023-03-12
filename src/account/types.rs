use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub balance: u64,
    pub max_transfer_amount: u64,
}

#[derive(Serialize, PartialEq, Clone)]
pub enum HistoryTxType {
    Deposit,
    Withdrawal,
    TransferIn,
    TransferOut,
    ReturnedChange,
    AggregateNotes,
    DirectDeposit,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTx {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    pub timestamp: u64,
    pub amount: u64,
    pub fee: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}