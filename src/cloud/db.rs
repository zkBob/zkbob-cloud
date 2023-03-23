use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::{errors::CloudError, helpers::db::KeyValueDb};

use super::types::{TransferPart, TransferTask, ReportTask, AccountData};

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

    pub fn account_exists(&self, id: Uuid) -> Result<bool, CloudError> {
        self.db.exists(CloudDbColumn::Accounts.into(), id.as_bytes())
    }

    pub fn delete_account(&mut self, id: Uuid) -> Result<(), CloudError> {
        self.db.delete(CloudDbColumn::Accounts.into(), id.as_bytes())
    }

    pub fn get_accounts(&self) -> Result<Vec<(Uuid, AccountData)>, CloudError> {
        let kv = self.db.get_all_with_keys(CloudDbColumn::Accounts.into())?;
        let mut accounts = Vec::new();
        for (id, data) in kv {
            let id = Uuid::from_slice(&id).map_err(|err| {
                tracing::error!("failed to parse account id: {:?}: {:?}", id, err);
                CloudError::DataBaseReadError("failed to parse account id".to_string())
            })?;
            accounts.push((id, data));
        }
        Ok(accounts)
    }

    pub fn save_task<'a, I>(
        &mut self,
        task: &TransferTask,
        parts: I,
    ) -> Result<(), CloudError> 
    where
        I: Iterator<Item = &'a TransferPart>,
    {
        self.db.save(
            CloudDbColumn::Tasks.into(),
            task.request_id.as_bytes(),
            task,
        )?;
        self.db.save_all(CloudDbColumn::Tasks.into(), parts, |part| part.id.as_bytes().to_vec())
    }

    pub fn get_task(&self, id: &str) -> Result<TransferTask, CloudError> {
        self.db
            .get(CloudDbColumn::Tasks.into(), id.as_bytes())?
            .ok_or(CloudError::InternalError("task not found in db".to_string()))
    }

    pub fn task_exists(&self, id: &str) -> Result<bool, CloudError> {
        self.db.exists(CloudDbColumn::Tasks.into(), id.as_bytes())
    }

    pub fn save_part(&mut self, part: &TransferPart) -> Result<(), CloudError> {
        self.db
            .save(CloudDbColumn::Tasks.into(), part.id.as_bytes(), part)
    }

    pub fn get_part(&self, id: &str) -> Result<TransferPart, CloudError> {
        self.db
            .get(CloudDbColumn::Tasks.into(), id.as_bytes())?
            .ok_or(CloudError::InternalError("task part not found in db".to_string()))
    }

    pub fn save_transaction_id(&mut self , tx_hash: &str, transaction_id: &str) -> Result<(), CloudError> {
        self.db.save_string(CloudDbColumn::TransactionId.into(), tx_hash.as_bytes(), transaction_id)
    }

    pub fn get_transaction_id(&self, tx_hash: &str) -> Result<Option<String>, CloudError> {
        self.db.get_string(CloudDbColumn::TransactionId.into(), tx_hash.as_bytes())
    }

    pub fn save_report_task(&mut self, id: Uuid, task: &ReportTask) -> Result<(), CloudError> {
        self.db.save(CloudDbColumn::Reports.into(), id.as_bytes(), task)
    }

    pub fn get_report_task(&self, id: Uuid) -> Result<Option<ReportTask>, CloudError> {
        self.db.get(CloudDbColumn::Reports.into(), id.as_bytes())
    }

    pub fn clean_reports(&mut self) -> Result<(), CloudError> {
        self.db.delete_all(CloudDbColumn::Reports.into())
    }
}

pub enum CloudDbColumn {
    Accounts,
    Tasks,
    TransactionId,
    Reports,
}

impl CloudDbColumn {
    pub fn count() -> u32 {
        4
    }
}

impl From<CloudDbColumn> for u32 {
    fn from(val: CloudDbColumn) -> Self {
        val as u32
    }
}
