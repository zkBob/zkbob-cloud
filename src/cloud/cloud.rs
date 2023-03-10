use std::{collections::HashMap, sync::Arc};

use libzkbob_rs::libzeropool::fawkes_crypto::ff_uint::Num;
use tokio::sync::RwLock;
use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::{account::{Account, types::AccountShortInfo}, config::Config, errors::CloudError, Fr, relayer::cached::CachedRelayerClient};

use super::db::Db;

pub struct ZkBobCloud {
    config: Config,
    db: RwLock<Db>,
    pool_id: Num<Fr>,
    
    relayer_fee: u64,
    relayer: Arc<CachedRelayerClient>,

    accounts: RwLock<HashMap<Uuid, Arc<Account>>>
}

impl ZkBobCloud {
    pub fn new(config: Config, pool_id: Num<Fr>) -> Result<Self, CloudError> {
        let db = Db::new(&config.db_path)?;
        let relayer = CachedRelayerClient::new(&config.relayer_url, &config.db_path)?;
        Ok(Self {
            config,
            db: RwLock::new(db),
            pool_id,
            relayer_fee: 10000, // TODO: fetch from relayer
            relayer: Arc::new(relayer),
            accounts: RwLock::new(HashMap::new())
        })
    }

    pub async fn new_account(&self, description: String, id: Option<Uuid>, sk: Option<Vec<u8>>) -> Result<Uuid, CloudError> {
        let id = id.unwrap_or(uuid::Uuid::new_v4());
        let db_path = self.db.read().await.account_db_path(id);
        let account = Account::new(id, description, sk, self.pool_id, &db_path).await?;
        let id = account.id;
        self.db.write().await.save_account(id, &db_path)?;
        tracing::info!("created a new account: {}", id);
        Ok(id)
    }

    pub async fn account_info(&self, id: Uuid) -> Result<AccountShortInfo, CloudError> {
        let account = self.get_account(id).await?;
        account.sync(self.relayer.clone()).await?;
        let info = account.short_info(self.relayer_fee).await;
        self.release_account(id).await;
        Ok(info)
    }

    pub async fn generate_address(&self, id: Uuid) -> Result<String, CloudError> {
        let account = self.get_account(id).await?;
        let address = account.generate_address().await;
        self.release_account(id).await;
        Ok(address)
    }


    pub fn validate_token(&self, bearer_token: &str) -> Result<(), CloudError> {
        if self.config.admin_token != bearer_token {
            return Err(CloudError::AccessDenied)
        }
        Ok(())
    }

    async fn get_account(&self, id: Uuid) -> Result<Arc<Account>, CloudError> {
        let db_path = self.db.read().await.get_account(id)?.ok_or(CloudError::AccountNotFound)?;

        let mut accounts = self.accounts.write().await;
        
        if accounts.contains_key(&id) {
            return Ok(accounts.get(&id).unwrap().clone())
        } 

        let account = Arc::new(Account::load(id, self.pool_id, &db_path)?);
        accounts.insert(id, account.clone());

        Ok(account)
    }

    async fn release_account(&self, id: Uuid) {
        let mut accounts = self.accounts.write().await;
        if accounts.contains_key(&id) {
            accounts.remove(&id);
        } 
    }
}