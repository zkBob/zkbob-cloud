use memo_parser::calldata::{ParsedCalldata, CalldataContent, transact::memo::TxType};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use web3::types::H256;
use zkbob_utils_rs::{contracts::{pool::Pool, dd::DdContract}, tracing};

use crate::errors::CloudError;

use super::db::Db;

#[derive(Serialize, Deserialize, Debug)]
pub enum Web3TxType {
    Deposit = 0,
    Transfer = 1,
    Withdrawal = 2,
    DepositPermittable = 3,
    DirectDeposit = 4,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TxWeb3Info {
    pub tx_type: Web3TxType,
    pub timestamp: u64,
    pub fee: Option<u64>,
    pub token_amount: Option<i128>,
}

pub struct CachedWeb3Client {
    pool: Pool,
    dd: DdContract,
    db: RwLock<Db>,
}

impl CachedWeb3Client {
    pub async fn new(pool: Pool, db_path: &str) -> Result<Self, CloudError> {
        let db = Db::new(db_path)?;
        let dd = pool.dd_contract().await?;
        Ok(CachedWeb3Client {
            pool,
            dd,
            db: RwLock::new(db),
        })
    }

    pub async fn get_web3_info(&self, tx_hash: &str) -> Result<TxWeb3Info, CloudError> {
        let info = {
            self.db.read().await.get_web3(tx_hash)
        };
        match info {
            Some(info) => Ok(info),
            None => {
                let info = self.fetch_web3_info(tx_hash).await?;
                if let Err(err) = self.db.write().await.save_web3(tx_hash, &info) {
                    tracing::warn!("failed to save web3 info for tx_hash: {}: {}", &tx_hash, err);
                }
                Ok(info)
            }
        }
    }
    
    async fn fetch_web3_info(&self, tx_hash: &str) -> Result<TxWeb3Info, CloudError> {
        let tx_hash: H256 = H256::from_slice(&hex::decode(&tx_hash[2..]).unwrap());
        let tx = self.pool
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
            CalldataContent::AppendDirectDeposit(_) => {
                let fee = self.dd.fee().await?;
                Ok((Web3TxType::DirectDeposit, Some(fee), None))
            }
            _ => Err(CloudError::InternalError("unknown tx".to_string())),
        }?;
    
        let timestamp = self.pool
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
}