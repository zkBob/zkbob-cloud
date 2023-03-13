use memo_parser::calldata::{transact::memo::TxType, CalldataContent, ParsedCalldata};
use serde::{Deserialize, Serialize};
use web3::types::H256;
use zkbob_utils_rs::contracts::pool::Pool;

use crate::errors::CloudError;

#[derive(Serialize, Deserialize)]
pub enum Web3TxType {
    Deposit = 0,
    Transfer = 1,
    Withdrawal = 2,
    DepositPermittable = 3,
    DirectDeposit = 4,
}

#[derive(Serialize, Deserialize)]
pub struct TxWeb3Info {
    pub tx_type: Web3TxType,
    pub timestamp: u64,
    pub fee: Option<u64>,
    pub token_amount: Option<i128>,
}

pub(crate) async fn get_web3_info(tx_hash: &str, pool: &Pool) -> Result<TxWeb3Info, CloudError> {
    let tx_hash: H256 = H256::from_slice(&hex::decode(&tx_hash[2..]).unwrap());
    let tx = pool
        .get_transaction(tx_hash)
        .await?
        .ok_or(CloudError::InternalError(
            "transaction not found".to_string(),
        ))?;

    let calldata = ParsedCalldata::new(tx.input.0.clone(), None).expect("Calldata is invalid!");
    let (tx_type, fee, token_amount) = match calldata.content {
        CalldataContent::Transact(calldata) => {
            let fee = calldata.memo.fee;
            let tx_type = match calldata.tx_type {
                TxType::Deposit => Web3TxType::Deposit,
                TxType::Transfer => Web3TxType::Transfer,
                TxType::Withdrawal => Web3TxType::Withdrawal,
                TxType::DepositPermittable => Web3TxType::DepositPermittable,
            };
            Ok((tx_type, Some(fee), Some(calldata.token_amount)))
        }
        CalldataContent::AppendDirectDeposit(calldata) => {
            // TODO: fetch fee somehow
            Ok((Web3TxType::DirectDeposit, None, None))
        }
        _ => Err(CloudError::InternalError("unknown tx".to_string())),
    }?;

    let timestamp = pool
        .block_timestamp(tx.block_number.unwrap())
        .await?
        .ok_or(CloudError::InternalError(
            "failed to fetch timestamp".to_string(),
        ))?
        .as_u64();

    Ok(TxWeb3Info {
        tx_type,
        timestamp,
        fee,
        token_amount,
    })
}
