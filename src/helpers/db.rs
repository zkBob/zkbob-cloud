use std::fmt::Debug;

use kvdb_rocksdb::DatabaseConfig;
use serde::{de::DeserializeOwned, Serialize};
use zkbob_utils_rs::tracing;

use crate::{Database, errors::CloudError};

pub struct KeyValueDb {
    path: String,
    db: Database
}

impl KeyValueDb {
    pub fn new(path: &str, columns: u32) -> Result<KeyValueDb, CloudError> {
        let db = Database::open(
            &DatabaseConfig {
                columns,
                ..Default::default()
            },
            path,
        )
        .map_err(|err| {
            tracing::error!("failed to open db [{}] with err: {:?}", path, err);
            CloudError::InternalError(format!("failed to open db"))
        })?;
        Ok(KeyValueDb { path: path.to_string(), db })
    }

    pub fn get<T: DeserializeOwned>(&self, column: u32, key: &[u8]) -> Result<Option<T>, CloudError> {
        let value = self.get_raw(column, key)?;
        match value {
            Some(value) => {
                Ok(Some(serde_json::from_slice(&value).map_err(|err| {
                    tracing::error!("failed to deserialize value [{:?}] from db: [{}] with err: {:?}", value, self.path, err);
                    CloudError::DataBaseReadError(format!("failed to deserialize value from db"))
                })?))
            },
            None => Ok(None)
        }
    }

    pub fn get_string(&self, column: u32, key: &[u8]) -> Result<Option<String>, CloudError> {
        let value = self.get_raw(column, key)?;
        match value {
            Some(value) => {
                Ok(Some(String::from_utf8(value).map_err(|err| {
                    tracing::error!("failed to deserialize value from db: [{}] with err: {:?}", self.path, err);
                    CloudError::DataBaseReadError(format!("failed to deserialize value from db"))
                })?))
            },
            None => Ok(None)
        }
    }

    pub fn get_raw(&self, column: u32, key: &[u8]) -> Result<Option<Vec<u8>>, CloudError> {
        self.db.get(column, key).map_err(|err| {
            tracing::error!("failed to get value [{}, {:?}] from db: [{}] with err: {:?}", column, key, self.path, err);
            CloudError::DataBaseReadError(format!("failed to get value from db"))
        })
    }

    pub fn get_all<T:DeserializeOwned>(&self, column: u32) -> Result<Vec<T>, CloudError> {
        let mut items = vec![];
        for (_, value) in self.db.iter(column) {
            let item = serde_json::from_slice(&value).map_err(|err| {
                tracing::error!("failed to deserialize value [{:?}] from db: [{}] with err: {:?}", value, self.path, err);
                CloudError::DataBaseReadError(format!("failed to deserialize value from db"))
            })?;
            items.push(item);
        }
        Ok(items)
    }

    pub fn get_all_with_keys<T:DeserializeOwned>(&self, column: u32) -> Result<Vec<(Vec<u8>, T)>, CloudError> {
        let mut items = vec![];
        for (key, value) in self.db.iter(column) {
            let item = serde_json::from_slice(&value).map_err(|err| {
                tracing::error!("failed to deserialize value [{:?}] from db: [{}] with err: {:?}", value, self.path, err);
                CloudError::DataBaseReadError(format!("failed to deserialize value from db"))
            })?;
            items.push((key.to_vec(), item));
        }
        Ok(items)
    }

    pub fn exists(&self, column: u32, key: &[u8]) -> Result<bool, CloudError> {
        Ok(self.get_raw(column, key)?.is_some())
    }

    pub fn save<T>(&mut self, column: u32, key: &[u8], value: &T) -> Result<(), CloudError> where T: Serialize + Debug {
        let value = serde_json::to_vec(value).map_err(|err| {
            tracing::error!("failed to serialize value [{:?}] for db: [{}] with err: {:?}", value, self.path, err);
            CloudError::DataBaseWriteError(format!("failed to serialize value"))
        })?;
        self.save_raw(column, key, &value)
    }

    pub fn save_string(&mut self, column: u32, key: &[u8], value: &str) -> Result<(), CloudError> {
        self.save_raw(column, key, value.as_bytes())
    }

    pub fn save_raw(&mut self, column: u32, key: &[u8], value: &[u8]) -> Result<(), CloudError> {
        self.db.write({
            let mut tx = self.db.transaction();
            tx.put(column, key, value);
            tx
        }).map_err(|err| {
            tracing::error!("failed to save value [{}, {:?}] in db: [{}] with err: {:?}", column, key, self.path, err);
            CloudError::DataBaseWriteError(format!("failed to save value"))
        })
    }

    pub fn save_all<T>(&mut self, column: u32, kv: Vec<(Vec<u8>, T)>) -> Result<(), CloudError> where T: Serialize + Debug {
        let mut tx = self.db.transaction();
        for (key, value) in kv {
            let value = serde_json::to_vec(&value).map_err(|err| {
                tracing::error!("failed to serialize value [{:?}] for db: [{}] with err: {:?}", value, self.path, err);
                CloudError::DataBaseWriteError(format!("failed to serialize value"))
            })?;
            tx.put_vec(column, &key, value);
        }
        self.db.write(tx).map_err(|err| {
            tracing::error!("failed to save tx [{}] in db: [{}] with err: {:?}", column, self.path, err);
            CloudError::DataBaseWriteError(format!("failed to save values"))
        })
    }
}