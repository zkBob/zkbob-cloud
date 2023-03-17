use libzkbob_rs::{libzeropool::{fawkes_crypto::ff_uint::Num, native::account::Account}, address::format_address};
use serde::Serialize;

use crate::{web3::cached::{TxWeb3Info, Web3TxType}, Fr, helpers::AsU64Amount, PoolParams};

use super::tx_parser::DecMemo;

#[derive(Serialize, PartialEq, Clone)]
pub enum HistoryTxType {
    Deposit,
    Withdrawal,
    TransferIn,
    TransferOut,
    ReturnedChange,
    AggregateNotes,
    DirectDeposit,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTx {
    pub tx_type: HistoryTxType,
    pub tx_hash: String,
    pub timestamp: u64,
    pub amount: u64,
    pub fee: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}

impl HistoryTx {
    pub(crate) fn parse(memo: DecMemo, info: TxWeb3Info, transaction_id: Option<String>, last_account: Option<Account<Fr>>) -> Vec<HistoryTx> {
        let tx_hash = memo.tx_hash.clone().unwrap();
        let mut history = vec![];
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
                    transaction_id,
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
                    transaction_id, 
                });
            }
            Web3TxType::Transfer => {
                if memo.in_notes.is_empty() && memo.out_notes.is_empty() {
                    let amount = {
                        let previous_amount = match last_account {
                            Some(acc) => *acc.b.as_num(),
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
                        transaction_id: transaction_id.clone(), 
                    });
                }

                for note in memo.in_notes.iter() {
                    let loopback = memo
                        .out_notes
                        .iter()
                        .any(|out_note| out_note.index == note.index);

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
                        transaction_id: transaction_id.clone(), 
                    });
                }

                let out_notes = memo.out_notes.iter().filter(|out_note| {
                    !memo
                        .in_notes
                        .iter().any(|in_note| in_note.index == out_note.index)                        
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
                        transaction_id: transaction_id.clone(), 
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
                    transaction_id, 
                });
            },
            Web3TxType::DirectDeposit => {
                let fee = info.fee.unwrap();
                for note in memo.in_notes.iter() {
                    let address =
                        format_address::<PoolParams>(note.note.d, note.note.p_d);

                    history.push(HistoryTx { 
                        tx_type: HistoryTxType::DirectDeposit, 
                        tx_hash: tx_hash.clone(), 
                        timestamp: info.timestamp, 
                        amount: note.note.b.to_num().as_u64_amount(), 
                        fee,
                        to: Some(address), 
                        transaction_id: transaction_id.clone(), 
                    });
                }
            }
        };
        history
    }
}