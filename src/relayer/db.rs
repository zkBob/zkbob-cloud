use libzkbob_rs::libzeropool::constants;

use crate::{errors::CloudError, helpers::db::KeyValueDb};

use super::cached::Transaction;

pub struct Db {
    db: KeyValueDb,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        Ok(Db {
            db: KeyValueDb::new(
                &format!("{}/relayer_cache", db_path),
                CacheDbColumn::count(),
            )?,
        })
    }

    pub fn save_txs(&mut self, txs: Vec<Transaction>) -> Result<(), CloudError> {
        let kv = txs
            .into_iter()
            .map(|tx| (tx.index.to_be_bytes().to_vec(), tx))
            .collect();
        self.db.save_all(CacheDbColumn::Transactions.into(), kv)
    }

    pub fn get_txs(&self, offset: u64, limit: u64) -> Vec<Transaction> {
        let mut result = Vec::new();
        for index in
            (offset..limit * (constants::OUT as u64 + 1) + offset).step_by(constants::OUT + 1)
        {
            match self
                .db
                .get(CacheDbColumn::Transactions.into(), &index.to_be_bytes())
            {
                Ok(Some(tx)) => result.push(tx),
                _ => break,
            }
        }
        result
    }
}

pub enum CacheDbColumn {
    Transactions,
}

impl CacheDbColumn {
    fn count() -> u32 {
        1
    }
}

impl Into<u32> for CacheDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}
