use std::{sync::Arc, panic::{self, AssertUnwindSafe}};

use libzkbob_rs::{
    client::{state::State, UserAccount, TxOutput, TokenAmount, TxType, TransactionData, StateFragment},
    libzeropool::{
        fawkes_crypto::{ff_uint::{Num, NumRepr}, rand::Rng, BorshSerialize},
        POOL_PARAMS, constants,
        native::account::Account as NativeAccount,
    },
    random::CustomRng, address::format_address,
};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{errors::CloudError, Database, Fr, PoolParams, helpers::AsU64Amount, relayer::cached::CachedRelayerClient, web3::cached::{CachedWeb3Client, Web3TxType}};

use self::{db::Db, types::{AccountInfo, HistoryTx, HistoryTxType}, tx_parser::StateUpdate};

pub mod types;
mod tx_parser;
mod db;

pub struct Account {
    pub id: Uuid,
    pub description: String,

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
            db: RwLock::new(db),
            inner: RwLock::new(inner),
        })
    }

    pub async fn export_key(&self) -> Result<String, CloudError> {
        let inner = self.inner.read().await;
        let sk_bytes = inner.keys.sk.try_to_vec().map_err(|e| {
            CloudError::InternalError(format!("failed to serialize private key {:#?}", e))
        })?;
        Ok(hex::encode(sk_bytes))
    }
    
    pub async fn next_index(&self) -> u64 {
        let inner = self.inner.read().await;
        inner.state.tree.next_index()
    }

    pub async fn info(&self, fee: u64) -> AccountInfo {
        let balance = {
            self.inner.read().await.state.total_balance().as_u64_amount()
        };

        AccountInfo {
            id: self.id.to_string(),
            description: self.description.clone(),
            balance,
            max_transfer_amount: self.max_transfer_amount(fee).await,
            address: self.generate_address().await,
        }
    }

    pub async fn generate_address(&self) -> String {
        let inner = self.inner.read().await;
        inner.generate_address()
    }

    pub async fn get_tx_parts(
        &self,
        total_amount: u64,
        fee: u64,
        to: &str,
    ) -> Result<Vec<(Option<String>, Num<Fr>)>, CloudError> {
        let account = self.inner.read().await;
        let amount = Num::from_uint(NumRepr::from(total_amount)).unwrap();
        let fee = Num::from_uint(NumRepr::from(fee)).unwrap();

        let mut account_balance = account.state.account_balance();
        let mut parts = vec![];

        if account_balance.to_uint() >= (amount + fee).to_uint() {
            parts.push((Some(to.to_string()), amount));
            return Ok(parts);
        }

        let notes = account.state.get_usable_notes();
        let mut balance_is_sufficient = false;
        for notes in notes.chunks(3) {
            let mut note_balance = Num::ZERO;
            for (_, note) in notes {
                note_balance += note.b.as_num();
            }

            if (note_balance + account_balance).to_uint() >= (amount + fee).to_uint() {
                parts.push((Some(to.to_string()), amount));
                balance_is_sufficient = true;
                break;
            } else {
                parts.push((None, note_balance - fee));
                account_balance += note_balance - fee;
            }
        }

        if !balance_is_sufficient {
            return Err(CloudError::InsufficientBalance);
        }

        Ok(parts)
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
        self.db.write().await.save_memos(parse_result.decrypted_memos)?;
        Ok(())
    }

    pub async fn create_transfer(&self, amount: Num<Fr>, to: Option<String>, fee: u64, relayer: Arc<CachedRelayerClient>) -> Result<TransactionData<Fr>, CloudError> {
        let tx_outputs = match to {
            Some(to) => {
                vec![TxOutput {
                    to: to.clone(),
                    amount: TokenAmount::new(amount),
                }]
            }
            None => vec![],
        };
        let fee = Num::from_uint(NumRepr::from(fee)).unwrap();
        let transfer = TxType::Transfer(TokenAmount::new(fee), vec![], tx_outputs);
        
        let extra_state = self.get_optimistic_state(relayer).await?;
        let account = self.inner.read().await;
        let tx = panic::catch_unwind(AssertUnwindSafe(|| {
            account
                .create_tx(transfer, None, Some(extra_state))
                .map_err(|e| CloudError::BadRequest(e.to_string()))
        }))
        .map_err(|_| {
            CloudError::InternalError("create tx panicked".to_string())
        })??;

        Ok(tx)
    }

    pub async fn history(&self, web3: Arc<CachedWeb3Client>) -> Result<Vec<HistoryTx>, CloudError> {
        let memos = {
            self.db.read().await.get_memos()?
        };

        let mut last_account: Option<NativeAccount<Fr>> = None;
        let mut history = vec![];
        for memo in memos {
            let tx_hash = memo.tx_hash.clone().unwrap();
            let info = web3.get_web3_info(&tx_hash).await?;
            match info.tx_type {
                Web3TxType::Deposit => {
                    let token_amount = info.token_amount.unwrap();
                    history.push(HistoryTx { 
                        tx_type: HistoryTxType::Deposit, 
                        tx_hash, 
                        timestamp: info.timestamp, 
                        amount: token_amount as u64, 
                        fee: info.fee.unwrap(), 
                        to: None, 
                        transaction_id: None, 
                    });
                }
                Web3TxType::DepositPermittable => {
                    let token_amount = info.token_amount.unwrap();
                    history.push(HistoryTx { 
                        tx_type: HistoryTxType::Deposit, 
                        tx_hash, 
                        timestamp: info.timestamp, 
                        amount: token_amount as u64, 
                        fee: info.fee.unwrap(), 
                        to: None, 
                        transaction_id: None, 
                    });
                }
                Web3TxType::Transfer => {
                    if memo.in_notes.len() == 0 && memo.out_notes.len() == 0 {
                        let amount = {
                            let previous_amount = match last_account {
                                Some(acc) => acc.b.as_num().clone(),
                                None => Num::ZERO,
                            };
                            memo.acc.unwrap().b.as_num() - previous_amount
                        };

                        history.push(HistoryTx { 
                            tx_type: HistoryTxType::AggregateNotes, 
                            tx_hash: tx_hash.clone(), 
                            timestamp: info.timestamp, 
                            amount: amount.as_u64_amount(), 
                            fee: info.fee.unwrap(), 
                            to: None, 
                            transaction_id: None, 
                        });
                    }

                    for note in memo.in_notes.iter() {
                        let loopback = memo
                            .out_notes
                            .iter()
                            .find(|out_note| out_note.index == note.index)
                            .is_some();

                        let tx_type = if loopback {
                            HistoryTxType::ReturnedChange
                        } else {
                            HistoryTxType::TransferIn
                        };
                        let address =
                            format_address::<PoolParams>(note.note.d, note.note.p_d);

                        history.push(HistoryTx { 
                            tx_type, 
                            tx_hash: tx_hash.clone(), 
                            timestamp: info.timestamp, 
                            amount: note.note.b.to_num().as_u64_amount(), 
                            fee: info.fee.unwrap(), 
                            to: Some(address), 
                            transaction_id: None, 
                        });
                    }

                    let out_notes = memo.out_notes.iter().filter(|out_note| {
                        memo
                            .in_notes
                            .iter()
                            .find(|in_note| in_note.index == out_note.index)
                            .is_none()
                    });
                    for note in out_notes {
                        let address =
                            format_address::<PoolParams>(note.note.d, note.note.p_d);

                        history.push(HistoryTx { 
                            tx_type: HistoryTxType::TransferOut, 
                            tx_hash: tx_hash.clone(), 
                            timestamp: info.timestamp, 
                            amount: note.note.b.to_num().as_u64_amount(), 
                            fee: info.fee.unwrap(), 
                            to: Some(address), 
                            transaction_id: None, 
                        });
                    }
                }
                Web3TxType::Withdrawal => {
                    let fee = info.fee.unwrap();
                    let token_amount = info.token_amount.unwrap();
                    history.push(HistoryTx { 
                        tx_type: HistoryTxType::Withdrawal, 
                        tx_hash, 
                        timestamp: info.timestamp, 
                        amount: (-(fee as i128 + token_amount)) as u64, 
                        fee: info.fee.unwrap(), 
                        to: None, 
                        transaction_id: None, 
                    });
                },
                Web3TxType::DirectDeposit => {
                    for note in memo.in_notes.iter() {
                        let address =
                            format_address::<PoolParams>(note.note.d, note.note.p_d);

                        history.push(HistoryTx { 
                            tx_type: HistoryTxType::DirectDeposit, 
                            tx_hash: tx_hash.clone(), 
                            timestamp: info.timestamp, 
                            amount: note.note.b.to_num().as_u64_amount(), 
                            fee: 0, // TODO: fetch fee 
                            to: Some(address), 
                            transaction_id: None, 
                        });
                    }
                }
            };

            if let Some(acc) = memo.acc {
                last_account = Some(acc);
            }
        }
        Ok(history)
    }

    pub async fn max_transfer_amount(
        &self,
        fee: u64,
    ) -> u64 {
        let fee = Num::from_uint(NumRepr::from(fee)).unwrap();

        let (mut account_balance, notes) = {
            let account = self.inner.read().await;
            (account.state.account_balance(), account.state.get_usable_notes())
        };
        
        let mut max_amount = if account_balance.to_uint() > fee.to_uint() {
            account_balance - fee
        } else {
            Num::ZERO
        };

        for notes in notes.chunks(3) {
            let mut note_balance = Num::ZERO;
            for (_, note) in notes {
                note_balance += note.b.as_num();
            }

            if (account_balance + note_balance).to_uint() < fee.to_uint() {
                break;
            }

            account_balance += note_balance - fee;
            if account_balance.to_uint() > max_amount.to_uint() {
                max_amount = account_balance;
            }
        }

        max_amount.as_u64_amount()
    }

    async fn get_optimistic_state(&self, relayer: Arc<CachedRelayerClient>) -> Result<StateFragment<Fr>, CloudError> {
        let (account_index, eta, params) = {
            let inner = self.inner.read().await;
            (inner.state.tree.next_index(), &inner.keys.eta.clone(), &inner.params.clone())
        };
        let relayer_index = relayer.info().await?.optimistic_delta_index;

        let limit = (relayer_index - account_index) / (constants::OUT as u64 + 1);
        let txs = relayer.transactions(account_index, limit, true).await?;
        let parse_result = tx_parser::parse_txs(txs, eta, params)?;
        Ok(StateFragment { 
            new_leafs: parse_result.state_update.new_leafs, 
            new_commitments: parse_result.state_update.new_commitments, 
            new_accounts: parse_result.state_update.new_accounts, 
            new_notes: parse_result.state_update.new_notes.into_iter().flatten().collect(), 
        })
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
