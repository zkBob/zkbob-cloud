pub mod types;
mod db;
mod queue;
mod send_worker;
mod status_worker;
mod cleanup;

use std::{collections::HashMap, sync::Arc};

use actix_web::web::Data;
use libzkbob_rs::libzeropool::fawkes_crypto::{backend::bellman_groth16::Parameters, ff_uint::Num};
use tokio::sync::RwLock;
use uuid::Uuid;
use zkbob_utils_rs::{contracts::pool::Pool, tracing};

use crate::{
    account::{history::HistoryTx, types::AccountInfo, Account},
    cloud::{
        db::AccountData,
        types::{TransferPart, TransferStatus, TransferTask},
    },
    config::Config,
    errors::CloudError,
    helpers::timestamp,
    relayer::cached::CachedRelayerClient,
    web3::cached::CachedWeb3Client,
    Engine, Fr,
};

use self::{db::Db, queue::Queue, send_worker::run_send_worker, status_worker::run_status_worker, types::{AccountShortInfo, Transfer}, cleanup::AccountCleanup};

pub struct ZkBobCloud {
    pub(crate) config: Config,
    pub(crate) db: RwLock<Db>,
    pub(crate) pool_id: Num<Fr>,
    pub(crate) params: Parameters<Engine>,

    pub(crate) relayer_fee: u64,
    pub(crate) relayer: Arc<CachedRelayerClient>,
    pub(crate) web3: Arc<CachedWeb3Client>,

    pub(crate) send_queue: Arc<RwLock<Queue>>,
    pub(crate) status_queue: Arc<RwLock<Queue>>,

    pub(crate) accounts: Arc<RwLock<HashMap<Uuid, Arc<Account>>>>,
}

impl ZkBobCloud {
    pub async fn new(
        config: Config,
        pool: Pool,
        pool_id: Num<Fr>,
        params: Parameters<Engine>,
    ) -> Result<Data<Self>, CloudError> {
        let db = Db::new(&config.db_path)?;
        let relayer = CachedRelayerClient::new(&config.relayer_url, &config.db_path)?;
        let relayer_fee = relayer.fee().await?;

        let web3 = CachedWeb3Client::new(pool, &config.db_path).await?;

        let send_queue = Arc::new(RwLock::new(
            Queue::new(
                "send",
                &config.redis_url,
                config.send_worker.queue_delay_sec,
                config.send_worker.queue_hidden_sec,
            )
            .await?,
        ));
        let status_queue = Arc::new(RwLock::new(
            Queue::new(
                "status",
                &config.redis_url,
                config.status_worker.queue_delay_sec,
                config.status_worker.queue_hidden_sec,
            )
            .await?,
        ));

        let cloud = Data::new(Self {
            config: config.clone(),
            db: RwLock::new(db),
            pool_id,
            params,
            relayer_fee,
            relayer: Arc::new(relayer),
            web3: Arc::new(web3),
            send_queue,
            status_queue: status_queue.clone(),
            accounts: Arc::new(RwLock::new(HashMap::new())),
        });

        run_send_worker(cloud.clone(), status_queue, config.send_worker.max_attempts).await?;
        run_status_worker(cloud.clone(), config.status_worker.max_attempts).await?;

        Ok(cloud)
    }

    pub async fn new_account(
        &self,
        description: String,
        id: Option<Uuid>,
        sk: Option<Vec<u8>>,
    ) -> Result<Uuid, CloudError> {
        let id = id.unwrap_or(uuid::Uuid::new_v4());
        let db_path = self.db.read().await.account_db_path(id);
        let account = Account::new(id, description.clone(), sk, self.pool_id, &db_path).await?;
        let id = account.id;
        self.db.write().await.save_account(
            id,
            &AccountData {
                db_path,
                description,
            },
        )?;
        tracing::info!("created a new account: {}", id);
        Ok(id)
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountShortInfo>, CloudError> {
        Ok(self
            .db
            .read()
            .await
            .get_accounts()?
            .into_iter()
            .map(|(id, data)| AccountShortInfo {
                id: id.as_hyphenated().to_string(),
                description: data.description,
            })
            .collect())
    }

    pub async fn account_info(&self, id: Uuid) -> Result<AccountInfo, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        account.sync(self.relayer.clone()).await?;
        let info = account.info(self.relayer_fee).await;
        Ok(info)
    }

    pub async fn generate_address(&self, id: Uuid) -> Result<String, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        let address = account.generate_address().await;
        Ok(address)
    }

    pub async fn history(&self, id: Uuid) -> Result<Vec<HistoryTx>, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        account.sync(self.relayer.clone()).await?;
        // TODO: optimistic history?
        let history = account.history(self.web3.clone()).await;
        history
    }

    pub async fn calculate_fee(&self, id: Uuid, amount: u64) -> Result<(u64, u64), CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        let parts = account
            .get_tx_parts(amount, self.relayer_fee, "dummy")
            .await?;
        Ok((parts.len() as u64, parts.len() as u64 * self.relayer_fee))
    }

    pub async fn export_key(&self, id: Uuid) -> Result<String, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        account.export_key().await
    }

    pub async fn transfer(&self, request: Transfer) -> Result<String, CloudError> {
        if request.id.contains('.') {
            return Err(CloudError::InvalidTransactionId);
        }

        if self.db.read().await.task_exists(&request.id)? {
            return Err(CloudError::DuplicateTransactionId);
        }

        let (account, _cleanup) = self.get_account(request.account_id).await?;
        account.sync(self.relayer.clone()).await?;

        let tx_parts = account
            .get_tx_parts(request.amount, self.relayer_fee, &request.to)
            .await?;

        let mut task = TransferTask {
            request_id: request.id.clone(),
            parts: Vec::new(),
        };
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
                timestamp: timestamp(),
            };
            parts.push(part);
            task.parts.push(format!("{}.{}", &request.id, i));
        }

        self.db.write().await.save_task(&task, parts.clone())?;

        let mut send_queue = self.send_queue.write().await;
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
            return Err(CloudError::AccessDenied);
        }
        Ok(())
    }

    pub(crate) async fn get_account(
        &self,
        id: Uuid,
    ) -> Result<(Arc<Account>, AccountCleanup), CloudError> {
        let data = self
            .db
            .read()
            .await
            .get_account(id)?
            .ok_or(CloudError::AccountNotFound)?;

        let mut accounts = self.accounts.write().await;

        if accounts.contains_key(&id) {
            return Ok((
                accounts.get(&id).unwrap().clone(),
                AccountCleanup {
                    id,
                    accounts: self.accounts.clone(),
                },
            ));
        }

        let account = Arc::new(Account::load(id, self.pool_id, &data.db_path)?);
        accounts.insert(id, account.clone());
        Ok((
            account,
            AccountCleanup {
                id,
                accounts: self.accounts.clone(),
            },
        ))
    }
}
