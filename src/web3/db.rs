use super::cached::TxWeb3Info;
use crate::{errors::CloudError, helpers::db::KeyValueDb};

pub struct Db {
    db: KeyValueDb,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        Ok(Db {
            db: KeyValueDb::new(&format!("{}/web3_cache", db_path), CacheDbCloumn::count())?,
        })
    }

    pub fn save_web3(&mut self, tx_hash: &str, web3: &TxWeb3Info) -> Result<(), CloudError> {
        self.db
            .save(CacheDbCloumn::Web3.into(), tx_hash.as_bytes(), web3)
    }

    pub fn get_web3(&self, tx_hash: &str) -> Option<TxWeb3Info> {
        self.db
            .get(CacheDbCloumn::Web3.into(), tx_hash.as_bytes())
            .ok()
            .flatten()
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
