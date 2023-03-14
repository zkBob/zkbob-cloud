use kvdb_rocksdb::DatabaseConfig;

use crate::{Database, errors::CloudError};

use super::cached::TxWeb3Info;


pub struct Db {
    db: Database,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        let db = Database::open(
            &DatabaseConfig {
                columns: CacheDbCloumn::count(),
                ..Default::default()
            },
            &format!("{}/web3_cache", db_path),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))?;

        Ok(Db {
            db,
        })
    }

    pub fn save_web3(&mut self, tx_hash: &str, web3: &TxWeb3Info) -> Result<(), CloudError> {
        let bytes = serde_json::to_vec(&web3).map_err(|err| CloudError::DataBaseWriteError(err.to_string()))?;
        self.db
            .write({
                let mut tx = self.db.transaction();
                tx.put_vec(CacheDbCloumn::Web3.into(), tx_hash.as_bytes(), bytes);
                tx
            })
            .map_err(|err| CloudError::DataBaseWriteError(err.to_string()))
    }

    pub fn get_web3(&self, tx_hash: &str) -> Option<TxWeb3Info> {
        let bytes = self.db.get(CacheDbCloumn::Web3.into(), tx_hash.as_bytes()).ok().flatten()?;
        serde_json::from_slice(&bytes).map_err(|err| CloudError::DataBaseReadError(err.to_string())).ok()
    }
}

pub enum CacheDbCloumn {
    Web3,
}

impl CacheDbCloumn {
    fn count() -> u32 {
        1
    }
}

impl Into<u32> for CacheDbCloumn {
    fn into(self) -> u32 {
        self as u32
    }
}