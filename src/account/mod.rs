use std::sync::Arc;

use libzkbob_rs::{
    client::{state::State, UserAccount},
    libzeropool::{
        fawkes_crypto::{ff_uint::Num, rand::Rng},
        POOL_PARAMS, constants,
    },
    random::CustomRng,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{errors::CloudError, Database, Fr, PoolParams, helpers::AsU64Amount, relayer::cached::CachedRelayerClient};

use self::{db::Db, types::AccountShortInfo, tx_parser::StateUpdate};

pub mod types;
mod db;
mod tx_parser;

pub struct Account {
    pub id: Uuid,
    pub description: String,
    pub(crate) sk: Vec<u8>,

    db: RwLock<Db>,
    inner: RwLock<UserAccount<Database, PoolParams>>,
}

impl Account {
    pub async fn new(
        id: Uuid,
        description: String,
        sk: Option<Vec<u8>>,
        pool_id: Num<Fr>,
        db_path: &str,
    ) -> Result<Self, CloudError> {
        let mut db = Db::new(db_path)?;
        let state = State::new(db.tree()?, db.txs()?);

        let sk = sk.unwrap_or_else(|| {
            let mut rng = CustomRng;
            rng.gen::<[u8; 32]>().to_vec()
        });
        let inner = UserAccount::from_seed(&sk, pool_id, state, POOL_PARAMS.clone());

        db.save_sk(&sk)?;
        db.save_description(&description)?;

        Ok(Self {
            id,
            description,
            sk: sk.to_vec(),
            db: RwLock::new(db),
            inner: RwLock::new(inner),
        })
    }

    pub fn load(id: Uuid, pool_id: Num<Fr>, db_path: &str) -> Result<Self, CloudError> {
        let db = Db::new(db_path)?;
        let state = State::new(db.tree()?, db.txs()?);

        let sk = db
            .get_sk()?
            .ok_or(CloudError::InternalError("failed to get sk".to_string()))?;
        let description = db.get_description()?.ok_or(CloudError::InternalError(
            "failed to get description".to_string(),
        ))?;

        let inner = UserAccount::from_seed(&sk, pool_id, state, POOL_PARAMS.clone());
        Ok(Self {
            id,
            description,
            sk,
            db: RwLock::new(db),
            inner: RwLock::new(inner),
        })
    }

    pub async fn next_index(&self) -> u64 {
        let inner = self.inner.read().await;
        inner.state.tree.next_index()
    }

    pub async fn short_info(&self, fee: u64) -> AccountShortInfo {
        let inner = self.inner.read().await;

        AccountShortInfo {
            id: self.id.to_string(),
            description: self.description.clone(),
            balance: inner.state.total_balance().as_u64_amount(),
            max_transfer_amount: 0, // TODO: fill it
        }
    }

    pub async fn generate_address(&self) -> String {
        let inner = self.inner.read().await;
        inner.generate_address()
    }

    pub async fn sync(&self, relayer: Arc<CachedRelayerClient>) -> Result<(), CloudError> {
        let (account_index, eta, params) = {
            let inner = self.inner.read().await;
            (inner.state.tree.next_index(), &inner.keys.eta.clone(), &inner.params.clone())
        };

        let relayer_index = relayer.info().await?.delta_index;

        let limit = (relayer_index - account_index) / (constants::OUT as u64 + 1);
        let txs = relayer.transactions(account_index, limit, false).await?;
        let parse_result = tx_parser::parse_txs(txs, eta, params)?;
        self.update_state(parse_result.state_update).await;
        // TODO: save history
        Ok(())
    }

    async fn update_state(&self, state_update: StateUpdate) {
        let mut inner = self.inner.write().await;
        if !state_update.new_leafs.is_empty() || !state_update.new_commitments.is_empty() {
            inner
                .state
                .tree
                .add_leafs_and_commitments(state_update.new_leafs, state_update.new_commitments);
        }

        state_update
            .new_accounts
            .into_iter()
            .for_each(|(at_index, account)| {
                inner.state.add_account(at_index, account);
            });

        state_update.new_notes.into_iter().for_each(|notes| {
            notes.into_iter().for_each(|(at_index, note)| {
                inner.state.add_note(at_index, note);
            });
        });
    }
}
