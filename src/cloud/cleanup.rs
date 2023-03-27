use std::{sync::Arc, collections::HashMap, thread, process};

use tokio::sync::RwLock;
use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::account::Account;

pub struct AccountCleanup {
    pub(crate) id: Uuid,
    pub(crate) accounts: Arc<RwLock<HashMap<Uuid, Arc<Account>>>>
}

impl AccountCleanup {
    pub fn new(id: Uuid, accounts: Arc<RwLock<HashMap<Uuid, Arc<Account>>>>) -> AccountCleanup {
        AccountCleanup { id, accounts }
    }
}

impl Drop for AccountCleanup {
    fn drop(&mut self) {
        let id = self.id;
        let accounts = self.accounts.clone();
        tokio::spawn(async move {
            accounts.write().await.remove(&id);
        });
    }
}

pub struct WorkerCleanup;

impl Drop for WorkerCleanup {
    fn drop(&mut self) {
        if thread::panicking() {
            tracing::error!("panic in worker, stopping application");
            process::exit(1);
        }
    }
}