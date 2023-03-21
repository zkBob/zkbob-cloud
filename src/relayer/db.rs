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

    pub fn save_txs<'a, I>(&mut self, txs: I) -> Result<(), CloudError>
    where
        I: Iterator<Item = &'a Transaction>,
    {
        self.db
            .save_all(CacheDbColumn::Transactions.into(), txs, |tx| {
                tx.index.to_be_bytes().to_vec()
            })
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

impl From<CacheDbColumn> for u32 {
    fn from(val: CacheDbColumn) -> Self {
        val as u32
    }
}
