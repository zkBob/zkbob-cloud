use kvdb_rocksdb::DatabaseConfig;
use uuid::Uuid;

use crate::{errors::CloudError, Database};

pub(crate) struct Db {
    db_path: String,
    db: Database,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        let db = Database::open(
            &DatabaseConfig {
                columns: CloudDbColumn::count(),
                ..Default::default()
            },
            &format!("{}/custody", db_path),
        )
        .map_err(|err| CloudError::InternalError(err.to_string()))?;

        Ok(Db { db_path: db_path.to_string(), db })
    }

    pub fn save_account(&mut self, id: Uuid, db_path: &str) -> Result<(), CloudError> {
        self.save(CloudDbColumn::Accounts, &id.as_hyphenated().to_string(), db_path.as_bytes())
    }

    pub fn get_account(&self, id: Uuid) -> Result<Option<String>, CloudError> {
        self.get(CloudDbColumn::Accounts, &id.as_hyphenated().to_string())
            .map(|opt| opt.map(|bytes| String::from_utf8(bytes).unwrap()))
    }

    pub fn account_db_path(&self, id: Uuid) -> String {
        format!("{}/accounts_data/{}", self.db_path, id.as_hyphenated())
    }

    fn save(&mut self, column: CloudDbColumn, key: &str, value: &[u8]) -> Result<(), CloudError> {
        self.db
            .write({
                let mut tx = self.db.transaction();
                tx.put(column.into(), key.as_bytes(), value);
                tx
            })
            .map_err(|err| CloudError::DataBaseWriteError(err.to_string()))
    }

    fn get(&self, column: CloudDbColumn, key: &str) -> Result<Option<Vec<u8>>, CloudError> {
        self.db
            .get(column.into(), key.as_bytes())
            .map_err(|err| CloudError::DataBaseReadError(err.to_string()))
    }
}

pub enum CloudDbColumn {
    Accounts,
    JobsIndex,
    TxRequestIndex,
    NullifierIndex,
}

impl CloudDbColumn {
    pub fn count() -> u32 {
        4
    }
}

impl Into<u32> for CloudDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}
