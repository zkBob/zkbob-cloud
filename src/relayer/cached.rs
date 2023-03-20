use libzkbob_rs::libzeropool::fawkes_crypto::ff_uint::{Num, NumRepr, Uint};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use zkbob_utils_rs::{
    relayer::{
        client::RelayerClient,
        types::{InfoResponse, JobResponse, TransactionRequest, TransactionResponse},
    },
    tracing,
};

use crate::{errors::CloudError, Fr};

use super::db::Db;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transaction {
    pub index: u64,
    pub memo: Vec<u8>,
    pub commitment: Num<Fr>,
    pub tx_hash: String,
    pub optimistic: bool,
}

pub struct CachedRelayerClient {
    client: RelayerClient,
    db: RwLock<Db>,
}

impl CachedRelayerClient {
    pub fn new(relayer_url: &str, db_path: &str) -> Result<Self, CloudError> {
        let client = RelayerClient::new(relayer_url)?;
        let db = Db::new(db_path)?;
        Ok(CachedRelayerClient {
            client,
            db: RwLock::new(db),
        })
    }

    pub async fn info(&self) -> Result<InfoResponse, CloudError> {
        Ok(self.client.info().await?)
    }

    pub async fn fee(&self) -> Result<u64, CloudError> {
        Ok(self.client.fee().await?)
    }

    pub async fn job(&self, id: &str) -> Result<JobResponse, CloudError> {
        Ok(self.client.job(id).await?)
    }

    pub async fn send_transactions(
        &self,
        request: Vec<TransactionRequest>,
    ) -> Result<TransactionResponse, CloudError> {
        Ok(self.client.send_transactions(request).await?)
    }

    pub async fn transactions(
        &self,
        offset: u64,
        limit: u64,
        with_optimistic: bool,
    ) -> Result<Vec<Transaction>, CloudError> {
        let cached = {
            let db = self.db.read().await;
            db.get_txs(offset, limit)
        };
        let offset = offset + 128 * cached.len() as u64;
        let limit = limit - cached.len() as u64;
        tracing::info!("cached: {}", cached.len());

        if limit == 0 {
            return Ok(cached);
        }

        let fetched = self.client.transactions(offset, limit).await?;
        tracing::info!("fetched: {}", fetched.len());

        let mut result = cached;
        for (i, tx) in fetched.into_iter().enumerate() {
            let index = offset + i as u64 * 128;
            let optimistic = &tx[0..1] != "1";
            let tx_hash = format!("0x{}", &tx[1..65]);
            let commitment: Num<Fr> = Num::from_uint_reduced(NumRepr(Uint::from_big_endian(
                &hex::decode(&tx[65..129]).unwrap(),
            )));
            let memo = hex::decode(&tx[129..]).unwrap();

            let tx = Transaction {
                index,
                memo,
                commitment,
                tx_hash,
                optimistic,
            };

            if with_optimistic || !optimistic {
                result.push(tx.clone());
            }
        }

        let new_mined = result.iter().filter(|tx| !tx.optimistic);
        let mut db = self.db.write().await;
        if db.save_txs(new_mined).is_err() {
            tracing::warn!("failed to save transactions");
        }

        Ok(result)
    }
}
