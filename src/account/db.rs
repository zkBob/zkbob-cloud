use kvdb_rocksdb::DatabaseConfig;
use libzkbob_rs::{
    client::state::Transaction, libzeropool::POOL_PARAMS, merkle::MerkleTree,
    sparse_array::SparseArray,
};

use crate::{errors::CloudError, Database, Fr, PoolParams};

use super::tx_parser::DecMemo;

pub(crate) struct Db {
    db_path: String,

    db: Database,
    history: Database,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        let db = Database::open(
            &DatabaseConfig {
                columns: AccountDbColumn::count(),
                ..Default::default()
            },
            &format!("{}/{}", db_path, "account"),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))?;

        let history = Database::open(
            &DatabaseConfig {
                columns: HistoryDbColumn::count(),
                ..Default::default()
            },
            &format!("{}/{}", db_path, "history"),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))?;

        Ok(Db {
            db_path: db_path.to_string(),
            db,
            history,
        })
    }

    pub fn save_sk(&mut self, sk: &[u8]) -> Result<(), CloudError> {
        self.save_db("sk", sk)
    }

    pub fn get_sk(&self) -> Result<Option<Vec<u8>>, CloudError> {
        self.get_db("sk")
    }

    pub fn save_description(&mut self, description: &str) -> Result<(), CloudError> {
        self.save_db("description", description.as_bytes())
    }

    pub fn get_description(&self) -> Result<Option<String>, CloudError> {
        self.get_db("description")
            .map(|opt| opt.map(|bytes| String::from_utf8(bytes).unwrap()))
    }

    pub fn tree(&self) -> Result<MerkleTree<Database, PoolParams>, CloudError> {
        MerkleTree::new_native(
            Default::default(),
            &format!("{}/{}", self.db_path, "tree"),
            POOL_PARAMS.clone(),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))
    }

    pub fn txs(&self) -> Result<SparseArray<Database, Transaction<Fr>>, CloudError> {
        SparseArray::new_native(
            &Default::default(),
            &format!("{}/{}", self.db_path, "txs"),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))
    }

    pub fn save_memos(&mut self, memos: Vec<DecMemo>) -> Result<(), CloudError> {
        self.history.write({
            let mut tx = self.history.transaction();
            for memo in memos {
                let key = &memo.index.to_be_bytes();
                let memo = serde_json::to_vec(&memo)
                    .map_err(|err| CloudError::InternalError(err.to_string()))?;
                tx.put_vec(HistoryDbColumn::Memo.into(), key, memo);
            }
            tx
        }).map_err(|err| CloudError::DataBaseWriteError(err.to_string()))
    }

    pub fn get_memos(&self) -> Result<Vec<DecMemo>, CloudError> {
        let mut memos = vec![];
        for (_, v) in self.history.iter(HistoryDbColumn::Memo.into()) {
            let memo: DecMemo = serde_json::from_slice(&v)
                .map_err(|err| CloudError::DataBaseReadError(err.to_string()))?;
            memos.push(memo);
        }
        Ok(memos)
    }

    fn save_db(&mut self, key: &str, value: &[u8]) -> Result<(), CloudError> {
        self.db
            .write({
                let mut tx = self.db.transaction();
                tx.put(AccountDbColumn::General.into(), key.as_bytes(), value);
                tx
            })
            .map_err(|err| CloudError::DataBaseWriteError(err.to_string()))
    }

    fn get_db(&self, key: &str) -> Result<Option<Vec<u8>>, CloudError> {
        self.db
            .get(AccountDbColumn::General.into(), key.as_bytes())
            .map_err(|err| CloudError::DataBaseReadError(err.to_string()))
    }

    fn save_history(&mut self, column: HistoryDbColumn, key: &[u8], value: &[u8]) -> Result<(), CloudError> {
        self.db
            .write({
                let mut tx = self.db.transaction();
                tx.put(column.into(), key, value);
                tx
            })
            .map_err(|err| CloudError::DataBaseWriteError(err.to_string()))
    }

    fn get_history(&self, column: HistoryDbColumn, key: &[u8]) -> Result<Option<Vec<u8>>, CloudError> {
        self.db
            .get(column.into(), key)
            .map_err(|err| CloudError::DataBaseReadError(err.to_string()))
    }
}

pub enum AccountDbColumn {
    General,
}

impl AccountDbColumn {
    fn count() -> u32 {
        1
    }
}

impl Into<u32> for AccountDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}

pub enum HistoryDbColumn {
    Memo,
    Web3,
}

impl HistoryDbColumn {
    fn count() -> u32 {
        2
    }
}

impl Into<u32> for HistoryDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}
