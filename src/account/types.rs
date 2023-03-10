use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct AccountShortInfo {
    pub id: String,
    pub description: String,
    pub balance: u64,
    pub max_transfer_amount: u64,
}