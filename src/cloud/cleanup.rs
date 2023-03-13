use std::{sync::Arc, collections::HashMap};

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::account::Account;

pub struct AccountCleanup {
    pub(crate) id: Uuid,
    pub(crate) accounts: Arc<RwLock<HashMap<Uuid, Arc<Account>>>>
}

impl Drop for AccountCleanup {
    fn drop(&mut self) {
        let id = self.id;
        let accounts = self.accounts.clone();
        tokio::spawn(async move {
            let mut accounts = accounts.write().await;
            if accounts.contains_key(&id) {
                accounts.remove(&id);
            };
        });
    }
}