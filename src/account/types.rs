use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub id: String,
    pub description: String,
    pub balance: u64,
    pub max_transfer_amount: u64,
    pub address: String,
}