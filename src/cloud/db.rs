use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::{errors::CloudError, helpers::db::KeyValueDb};

use super::types::{TransferPart, TransferTask};

pub(crate) struct Db {
    db_path: String,
    db: KeyValueDb,
}

impl Db {
    pub fn new(db_path: &str) -> Result<Self, CloudError> {
        Ok(Db {
            db_path: db_path.to_string(),
            db: KeyValueDb::new(&format!("{}/cloud", db_path), CloudDbColumn::count())?,
        })
    }

    pub fn account_db_path(&self, id: Uuid) -> String {
        format!("{}/accounts_data/{}", self.db_path, id.as_hyphenated())
    }

    pub fn save_account(&mut self, id: Uuid, data: &AccountData) -> Result<(), CloudError> {
        self.db
            .save(CloudDbColumn::Accounts.into(), id.as_bytes(), data)
    }

    pub fn get_account(&self, id: Uuid) -> Result<Option<AccountData>, CloudError> {
        self.db.get(CloudDbColumn::Accounts.into(), id.as_bytes())
    }

    pub fn get_accounts(&self) -> Result<Vec<(Uuid, AccountData)>, CloudError> {
        let kv = self.db.get_all_with_keys(CloudDbColumn::Accounts.into())?;
        let mut accounts = Vec::new();
        for (id, data) in kv {
            let id = Uuid::from_slice(&id).map_err(|err| {
                tracing::error!("failed to parse account id: {:?}: {:?}", id, err);
                CloudError::DataBaseReadError(format!("failed to parse account id"))
            })?;
            accounts.push((id, data));
        }
        Ok(accounts)
    }

    pub fn save_task(
        &mut self,
        task: &TransferTask,
        parts: Vec<TransferPart>,
    ) -> Result<(), CloudError> {
        self.db.save(
            CloudDbColumn::Tasks.into(),
            task.request_id.as_bytes(),
            task,
        )?;
        let kv = parts
            .into_iter()
            .map(|part| (part.id.as_bytes().to_vec(), part))
            .collect();
        self.db.save_all(CloudDbColumn::Tasks.into(), kv)
    }

    pub fn get_task(&self, id: &str) -> Result<TransferTask, CloudError> {
        self.db
            .get(CloudDbColumn::Tasks.into(), id.as_bytes())?
            .ok_or(CloudError::InternalError(format!("task not found in db")))
    }

    pub fn task_exists(&self, id: &str) -> Result<bool, CloudError> {
        self.db.exists(CloudDbColumn::Tasks.into(), id.as_bytes())
    }

    pub fn save_part(&mut self, part: &TransferPart) -> Result<(), CloudError> {
        self.db
            .save(CloudDbColumn::Tasks.into(), &part.id.as_bytes(), part)
    }

    pub fn get_part(&self, id: &str) -> Result<TransferPart, CloudError> {
        self.db
            .get(CloudDbColumn::Tasks.into(), id.as_bytes())?
            .ok_or(CloudError::InternalError(format!(
                "task part not found in db"
            )))
    }
}

pub enum CloudDbColumn {
    Accounts,
    Tasks,
}

impl CloudDbColumn {
    pub fn count() -> u32 {
        2
    }
}

impl Into<u32> for CloudDbColumn {
    fn into(self) -> u32 {
        self as u32
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccountData {
    pub description: String,
    pub db_path: String,
}
