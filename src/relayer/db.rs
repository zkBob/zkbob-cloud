use kvdb_rocksdb::DatabaseConfig;

use crate::{errors::CloudError, Database};

use super::cached::Transaction;

pub struct Db {
    db: Database,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        let db = Database::open(
            &DatabaseConfig {
                columns: CacheDbColumn::count(),
                ..Default::default()
            },
            &format!("{}/relayer_cache", db_path),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))?;

        Ok(Db {
            db,
        })
    }

    pub fn save_txs(&mut self, txs: Vec<Transaction>) -> Result<(), CloudError> {
        let mut db_tx = self.db.transaction();
        txs.iter().for_each(|tx| {
            db_tx.put_vec(
                CacheDbColumn::Transactions.into(),
                &tx.index.to_be_bytes(),
                serde_json::to_vec(tx).unwrap(),
            )
        });
        self.db
            .write(db_tx)
            .map_err(|err| CloudError::DataBaseWriteError(err.to_string()))
    }

    pub fn get_txs(&self, offset: u64, limit: u64) -> Vec<Transaction> {
        let mut result = Vec::new();
        for index in (offset..limit * 128 + offset).step_by(128) {
            match self
                .db
                .get(CacheDbColumn::Transactions.into(), &index.to_be_bytes())
            {
                Ok(Some(tx)) => {
                    let tx = serde_json::from_slice::<Transaction>(&tx);
                    match tx {
                        Ok(tx) => result.push(tx),
                        Err(_) => break,
                    }
                }
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
