use memo_parser::calldata::{ParsedCalldata, CalldataContent, transact::memo::TxType};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use web3::types::H256;
use zkbob_utils_rs::{contracts::{pool::Pool, dd::DdContract}, tracing};

use crate::errors::CloudError;

use super::db::Db;

#[derive(Serialize, Deserialize, Debug)]
pub enum TxWeb3Info {
    Deposit(u64, u64, i128),
    Transfer(u64, u64, i128),
    Withdrawal(u64, u64, i128),
    DepositPermittable(u64, u64, i128),
    DirectDeposit(u64, u64),
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
        let tx_hash: H256 = H256::from_slice(&hex::decode(&tx_hash[2..])?);
        let tx = self.pool
            .get_transaction(tx_hash)
            .await?
            .ok_or(CloudError::InternalError(
                "transaction not found".to_string(),
            ))?;

        let block_number = tx.block_number.ok_or(CloudError::Web3Error)?;
        let timestamp = self.pool
            .block_timestamp(block_number)
            .await?
            .ok_or(CloudError::InternalError(
                "failed to fetch timestamp".to_string(),
            ))?
            .as_u64();
    
        let calldata = ParsedCalldata::new(tx.input.0, None).expect("Calldata is invalid!");
        match calldata.content {
            CalldataContent::Transact(calldata) => {
                let fee = calldata.memo.fee;
                match calldata.tx_type {
                    TxType::Deposit => Ok(TxWeb3Info::Deposit(timestamp, fee, calldata.token_amount)),
                    TxType::Transfer => Ok(TxWeb3Info::Transfer(timestamp, fee, calldata.token_amount)),
                    TxType::Withdrawal => Ok(TxWeb3Info::Withdrawal(timestamp, fee, calldata.token_amount)),
                    TxType::DepositPermittable => Ok(TxWeb3Info::DepositPermittable(timestamp, fee, calldata.token_amount)),
                }
            }
            CalldataContent::AppendDirectDeposit(_) => {
                let fee = self.dd.fee().await?;
                Ok(TxWeb3Info::DirectDeposit(timestamp, fee))
            }
            _ => Err(CloudError::InternalError("unknown tx".to_string())),
        }
    }
}