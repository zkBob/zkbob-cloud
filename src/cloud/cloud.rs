use std::{collections::HashMap, sync::Arc, time::Duration, thread};

use actix_web::web::Data;
use libzkbob_rs::libzeropool::fawkes_crypto::{ff_uint::Num, backend::bellman_groth16::Parameters};
use tokio::sync::RwLock;
use uuid::Uuid;
use zkbob_utils_rs::{tracing, contracts::pool::Pool};

use crate::{account::{Account, types::{AccountShortInfo, HistoryTx}}, config::Config, errors::CloudError, Fr, relayer::cached::CachedRelayerClient, cloud::types::{TransferTask, TransferPart, TransferStatus}, Engine};

use super::{db::Db, types::{TransferRequest, TransferResponse, Transfer}, queue::Queue, send_worker::run_send_worker, status_worker::run_status_worker};

pub struct ZkBobCloud {
    pub(crate) config: Config,
    pub(crate) db: RwLock<Db>,
    pub(crate) pool_id: Num<Fr>,
    pub(crate) params: Parameters<Engine>,
    
    pub(crate) relayer_fee: u64,
    pub(crate) relayer: Arc<CachedRelayerClient>,
    pub(crate) pool: Pool,

    pub(crate) send_queue: Arc<RwLock<Queue>>,
    pub(crate) status_queue: Arc<RwLock<Queue>>,

    pub(crate) accounts: RwLock<HashMap<Uuid, Arc<Account>>>
}

impl ZkBobCloud {
    pub async fn new(config: Config, pool: Pool, pool_id: Num<Fr>, params: Parameters<Engine>) -> Result<Data<Self>, CloudError> {
        let db = Db::new(&config.db_path)?;
        let relayer = CachedRelayerClient::new(&config.relayer_url, &config.db_path)?;

        let send_queue = Arc::new(RwLock::new(Queue::new("send", &config.redis_url).await?));
        let status_queue = Arc::new(RwLock::new(Queue::new("status", &config.redis_url).await?));

        let cloud = Data::new(Self {
            config,
            db: RwLock::new(db),
            pool_id,
            params,
            relayer_fee: 100000000, // TODO: fetch from relayer
            relayer: Arc::new(relayer),
            pool,
            send_queue,
            status_queue: status_queue.clone(),
            accounts: RwLock::new(HashMap::new())
        });

        run_send_worker(cloud.clone(), status_queue).await?;
        run_status_worker(cloud.clone()).await?;

        Ok(cloud)
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

    pub async fn history(&self, id: Uuid) -> Result<Vec<HistoryTx>, CloudError> {
        let account = self.get_account(id).await?;
        let history = account.history(&self.pool).await;
        self.release_account(id).await;
        history
    }

    pub async fn transfer(&self, request: Transfer) -> Result<String, CloudError> {
        let account = self.get_account(request.account_id).await?;
        account.sync(self.relayer.clone()).await?;

        let tx_parts = account.get_tx_parts(request.amount, self.relayer_fee, request.to).await?;
        
        let mut send_queue = self.send_queue.write().await;
        let mut task = TransferTask{ request_id: request.id.clone(), parts: Vec::new() };
        let mut parts = Vec::new();
        for (i, tx_part) in tx_parts.into_iter().enumerate() {
            let part = TransferPart {
                id: format!("{}.{}", &request.id, i),
                request_id: request.id.clone(),
                account_id: request.account_id.to_string(),
                amount: tx_part.1,
                fee: self.relayer_fee,
                to: tx_part.0,
                status: TransferStatus::New,
                job_id: None,
                tx_hash: None,
                depends_on: (i > 0).then_some(format!("{}.{}", &request.id, i - 1)),
                attempt: 0,
            };
            parts.push(part);
            task.parts.push(format!("{}.{}", &request.id, i));
        }

        self.db.write().await.save_task(task, &parts)?;
        for part in parts {
            send_queue.send(part.id).await?;
        }

        Ok(request.id)
    }

    pub async fn transfer_status(&self, id: &str) -> Result<Vec<TransferPart>, CloudError> {
        let db = self.db.read().await;
        let transfer = db.get_task(id)?;
        let mut parts = Vec::new();
        for id in transfer.parts {
            let part = db.get_part(&id)?;
            parts.push(part);
        }
        Ok(parts)
    }

    pub fn validate_token(&self, bearer_token: &str) -> Result<(), CloudError> {
        if self.config.admin_token != bearer_token {
            return Err(CloudError::AccessDenied)
        }
        Ok(())
    }

    pub(crate) async fn get_account(&self, id: Uuid) -> Result<Arc<Account>, CloudError> {
        let db_path = self.db.read().await.get_account(id)?.ok_or(CloudError::AccountNotFound)?;

        let mut accounts = self.accounts.write().await;
        
        if accounts.contains_key(&id) {
            return Ok(accounts.get(&id).unwrap().clone())
        } 

        let account = Arc::new(Account::load(id, self.pool_id, &db_path)?);
        accounts.insert(id, account.clone());

        Ok(account)
    }

    pub(crate) async fn release_account(&self, id: Uuid) {
        let mut accounts = self.accounts.write().await;
        if accounts.contains_key(&id) {
            accounts.remove(&id);
        } 
    }
}