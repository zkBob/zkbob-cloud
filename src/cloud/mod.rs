pub mod types;
mod db;
mod send_worker;
mod status_worker;
mod report_worker;
mod cleanup;

use std::{collections::HashMap, sync::Arc};

use actix_web::web::Data;
use libzkbob_rs::libzeropool::fawkes_crypto::{backend::bellman_groth16::Parameters, ff_uint::Num};
use tokio::{sync::RwLock, fs};
use uuid::Uuid;
use zkbob_utils_rs::{contracts::pool::Pool, tracing};

use crate::{
    account::{types::AccountInfo, Account},
    cloud::types::{TransferPart, TransferStatus, TransferTask, AccountData},
    config::Config,
    errors::CloudError,
    helpers::{timestamp, queue::Queue},
    relayer::cached::CachedRelayerClient,
    web3::cached::CachedWeb3Client,
    Engine, Fr,
};

use self::{db::Db, send_worker::run_send_worker, status_worker::run_status_worker, types::{AccountShortInfo, Transfer, ReportTask, ReportStatus, AccountImportData, CloudHistoryTx}, cleanup::AccountCleanup, report_worker::run_report_worker};

pub struct ZkBobCloud {
    pub(crate) config: Data<Config>,
    pub(crate) db: RwLock<Db>,
    pub(crate) pool_id: Num<Fr>,
    pub(crate) params: Arc<Parameters<Engine>>,

    pub(crate) relayer_fee: u64,
    pub(crate) relayer: CachedRelayerClient,
    pub(crate) web3: CachedWeb3Client,

    pub(crate) send_queue: Arc<RwLock<Queue>>,
    pub(crate) status_queue: Arc<RwLock<Queue>>,
    pub(crate) report_queue: Arc<RwLock<Queue>>,

    pub(crate) accounts: Arc<RwLock<HashMap<Uuid, Arc<Account>>>>,
}

impl ZkBobCloud {
    pub async fn new(
        config: Data<Config>,
        pool: Pool,
        pool_id: Num<Fr>,
        params: Parameters<Engine>,
    ) -> Result<Data<Self>, CloudError> {
        let db = Db::new(&config.db_path)?;
        let relayer = CachedRelayerClient::new(&config.relayer_url, &config.db_path)?;
        let relayer_fee = relayer.fee().await?;

        let web3 = CachedWeb3Client::new(pool, &config.db_path).await?;

        let send_queue = Queue::new(
            "send",
            &config.redis_url,
            config.send_worker.queue_delay_sec,
            config.send_worker.queue_hidden_sec,
        )
        .await?;

        let status_queue = Queue::new(
            "status",
            &config.redis_url,
            config.status_worker.queue_delay_sec,
            config.status_worker.queue_hidden_sec,
        )
        .await?;
            
        let report_queue = Queue::new("report", &config.redis_url, 0, 180).await?;

        let cloud = Data::new(Self {
            config: config.clone(),
            db: RwLock::new(db),
            pool_id,
            params: Arc::new(params),
            relayer_fee,
            relayer,
            web3,
            send_queue: Arc::new(RwLock::new(send_queue)),
            status_queue: Arc::new(RwLock::new(status_queue)),
            report_queue: Arc::new(RwLock::new(report_queue)),
            accounts: Arc::new(RwLock::new(HashMap::new())),
        });

        run_send_worker(cloud.clone(), config.send_worker.max_attempts);
        run_status_worker(cloud.clone(), config.status_worker.max_attempts);
        run_report_worker(cloud.clone(), 5);
        
        Ok(cloud)
    }

    pub async fn new_account(
        &self,
        description: String,
        id: Option<Uuid>,
        sk: Option<Vec<u8>>,
    ) -> Result<Uuid, CloudError> {
        let id = id.unwrap_or(uuid::Uuid::new_v4());
        if self.db.read().await.account_exists(id)? {
            return Err(CloudError::DuplicateAccountId);
        }

        let db_path = self.db.read().await.account_db_path(id);
        let account = Account::new(id, description.clone(), sk, self.pool_id, &db_path)?;
        let id = account.id;
        self.db.write().await.save_account(
            id,
            &AccountData {
                db_path,
                description,
                sk: account.export_key().await?,
            },
        )?;
        tracing::info!("created a new account: {}", id);
        Ok(id)
    }

    pub async fn import_accounts(&self, accounts: Vec<AccountImportData>) -> Result<(), CloudError> {
        for account in accounts {
            self.new_account(account.description, Some(account.id), Some(account.sk)).await?;
        }
        Ok(())
    }

    pub async fn delete_account(&self, id: Uuid) -> Result<(), CloudError> {
        let data = self.db.read().await
            .get_account(id)?
            .ok_or(CloudError::AccountNotFound)?;

        let accounts = self.accounts.write().await;
        if accounts.get(&id).is_some() {
            return Err(CloudError::AccountIsBusy);
        }

        fs::remove_dir_all(&data.db_path).await.map_err(|err| {
            tracing::warn!("failed to delete account data: {}", err);
            CloudError::InternalError("failed to delete account data".to_string())
        })?;

        self.db.write().await.delete_account(id)
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
                sk: data.sk,
            })
            .collect())
    }

    pub async fn account_info(&self, id: Uuid) -> Result<AccountInfo, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        account.sync(&self.relayer, None).await?;
        let info = account.info(self.relayer_fee).await;
        Ok(info)
    }

    pub async fn generate_address(&self, id: Uuid) -> Result<String, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        let address = account.generate_address().await;
        Ok(address)
    }

    pub async fn history(&self, id: Uuid) -> Result<Vec<CloudHistoryTx>, CloudError> {
        let (account, _cleanup) = self.get_account(id).await?;
        account.sync(&self.relayer, None).await?;
        // TODO: optimistic history?
        let history = account.history(&self.web3).await?;
        let mut result = vec![];
        for record in history {
            let transaction_id = self.db.read().await.get_transaction_id(&record.tx_hash)?;
            result.push(CloudHistoryTx::new(record, transaction_id));
        }
        Ok(result)
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
        account.sync(&self.relayer, None).await?;

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

        self.db.write().await.save_task(&task, parts.iter())?;

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

    pub async fn generate_report(&self) -> Result<(Uuid, ReportTask), CloudError> {
        let id = Uuid::new_v4();
        let task = ReportTask {
            status: ReportStatus::New,
            attempt: 0,
            report: None,
        };
        self.db.write().await.save_report_task(id, &task)?;
        self.report_queue.write().await.send(id.as_hyphenated().to_string()).await?;
        Ok((id, task))
    }

    pub async fn get_report(&self, id: Uuid) -> Result<Option<ReportTask>, CloudError> {
        self.db.read().await.get_report_task(id)
    }

    pub async fn clean_reports(&self) -> Result<(), CloudError> {
        self.db.write().await.clean_reports()
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
        match accounts.get(&id) {
            Some(account) => Ok((account.clone(), AccountCleanup::new(id, self.accounts.clone()))),
            None => {
                let account = Account::load(id, self.pool_id, &data.db_path).or_else(|_| {
                    let sk = hex::decode(data.sk)?;
                    Account::new(id, data.description, Some(sk), self.pool_id, &data.db_path)
                })?;
                let account = Arc::new(account);
                accounts.insert(id, account.clone());
                Ok((account, AccountCleanup::new(id, self.accounts.clone())))
            }
        }
    }
}
