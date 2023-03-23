use libzkbob_rs::{
    client::state::Transaction, libzeropool::POOL_PARAMS, merkle::MerkleTree,
    sparse_array::SparseArray,
};
use zkbob_utils_rs::tracing;

use crate::{errors::CloudError, helpers::db::KeyValueDb, Database, Fr, PoolParams};

use super::tx_parser::DecMemo;

pub(crate) struct Db {
    db_path: String,

    db: KeyValueDb,
    history: KeyValueDb,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        Ok(Db {
            db_path: db_path.to_string(),
            db: KeyValueDb::new(
                &format!("{}/{}", db_path, "account"),
                AccountDbColumn::count(),
            )?,
            history: KeyValueDb::new(
                &format!("{}/{}", db_path, "history"),
                HistoryDbColumn::count(),
            )?,
        })
    }

    pub fn tree(&self) -> Result<MerkleTree<Database, PoolParams>, CloudError> {
        let path = format!("{}/{}", self.db_path, "tree");
        MerkleTree::new_native(Default::default(), &path, POOL_PARAMS.clone()).map_err(|err| {
            tracing::error!("failed to init MerkleTree [{}]: {:?}", path, err);
            CloudError::InternalError("failed to init MerkleTree".to_string())
        })
    }

    pub fn txs(&self) -> Result<SparseArray<Database, Transaction<Fr>>, CloudError> {
        let path = format!("{}/{}", self.db_path, "txs");
        SparseArray::new_native(&Default::default(), &path).map_err(|err| {
            tracing::error!("failed to init SparceArray [{}]: {:?}", path, err);
            CloudError::InternalError("failed to init SparseArray".to_string())
        })
    }

    pub fn save_sk(&mut self, sk: &[u8]) -> Result<(), CloudError> {
        self.db
            .save_raw(AccountDbColumn::General.into(), "sk".as_bytes(), sk)
    }

    pub fn get_sk(&self) -> Result<Option<Vec<u8>>, CloudError> {
        self.db
            .get_raw(AccountDbColumn::General.into(), "sk".as_bytes())
    }

    pub fn save_description(&mut self, description: &str) -> Result<(), CloudError> {
        self.db.save_string(
            AccountDbColumn::General.into(),
            "description".as_bytes(),
            description,
        )
    }

    pub fn get_description(&self) -> Result<Option<String>, CloudError> {
        self.db
            .get_string(AccountDbColumn::General.into(), "description".as_bytes())
    }

    pub fn save_memos<'a, I>(&mut self, memos: I) -> Result<(), CloudError> 
    where
        I: Iterator<Item = &'a DecMemo>,
    {
        self.history.save_all(HistoryDbColumn::Memo.into(), memos, |memo| memo.index.to_be_bytes().to_vec())
    }

    pub fn get_memos(&self) -> Result<Vec<DecMemo>, CloudError> {
        self.history.get_all(HistoryDbColumn::Memo.into())
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

impl From<AccountDbColumn> for u32 {
    fn from(val: AccountDbColumn) -> Self {
        val as u32
    }
}

pub enum HistoryDbColumn {
    Memo
}

impl HistoryDbColumn {
    fn count() -> u32 {
        1
    }
}

impl From<HistoryDbColumn> for u32 {
    fn from(val: HistoryDbColumn) -> Self {
        val as u32
    }
}
