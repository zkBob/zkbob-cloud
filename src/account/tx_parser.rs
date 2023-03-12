// TODO: move tx_parser to libzkbob.rs and use that one

use libzkbob_rs::{libzeropool::{fawkes_crypto::ff_uint::{Num, NumRepr, Uint, byteorder::{ReadBytesExt, LittleEndian}}, native::{account::Account, note::Note, key::derive_key_p_d, cipher}, constants}, delegated_deposit::{MEMO_DELEGATED_DEPOSIT_SIZE, MemoDelegatedDeposit, DELEGATED_DEPOSIT_FLAG}, utils::zero_account, keys::Keys};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use serde::{Serialize, Deserialize};
use thiserror::Error;

use crate::{relayer::cached::Transaction, Fr, PoolParams, errors::CloudError};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Incorrect memo length at index {0}: no prefix")]
    NoPrefix(u64),
    #[error("Incorrect memo prefix at index {0}: got {1} items, max allowed {2}")]
    IncorrectPrefix(u64, u32, u32),
}

impl ParseError {
    pub fn index(&self) -> u64 {
        match *self {
            ParseError::NoPrefix(idx)  => idx,
            ParseError::IncorrectPrefix(idx,  _, _)  => idx,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IndexedNote {
    pub index: u64,
    pub note: Note<Fr>,
}

#[derive(Clone, Default, Debug)]
pub struct StateUpdate {
    pub new_leafs: Vec<(u64, Vec<Num<Fr>>)>,
    pub new_commitments: Vec<(u64, Num<Fr>)>,
    pub new_accounts: Vec<(u64, Account<Fr>)>,
    pub new_notes: Vec<Vec<(u64, Note<Fr>)>>
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct DecMemo {
    pub index: u64,
    pub acc: Option<Account<Fr>>,
    pub in_notes: Vec<IndexedNote>,
    pub out_notes: Vec<IndexedNote>,
    pub tx_hash: Option<String>,
}

#[derive(Default, Debug)]
pub struct ParseResult {
    pub decrypted_memos: Vec<DecMemo>,
    pub state_update: StateUpdate
}

pub fn parse_txs(txs: Vec<Transaction>, eta: &Num<Fr>, params: &PoolParams) -> Result<ParseResult, CloudError> {
    let (parse_results, parse_errors): (Vec<_>, Vec<_>) = txs.into_par_iter()
        .map(|tx| -> Result<ParseResult, ParseError> {
            parse_tx(tx, eta, params)
        })
        .partition(Result::is_ok);

    if parse_errors.is_empty() {
        let parse_result = parse_results
            .into_iter()
            .map(Result::unwrap)
            .fold(Default::default(), |acc: ParseResult, parse_result| {
                ParseResult {
                    decrypted_memos: vec![acc.decrypted_memos, parse_result.decrypted_memos].concat(),
                    state_update: StateUpdate {
                        new_leafs: vec![acc.state_update.new_leafs, parse_result.state_update.new_leafs].concat(),
                        new_commitments: vec![acc.state_update.new_commitments, parse_result.state_update.new_commitments].concat(),
                        new_accounts: vec![acc.state_update.new_accounts, parse_result.state_update.new_accounts].concat(),
                        new_notes: vec![acc.state_update.new_notes, parse_result.state_update.new_notes].concat()
                    }
                }
        });
        Ok(parse_result)
    } else {
        // let errors: Vec<_> = parse_errors
        //     .into_iter()
        //     .map(|err| -> ParseError {
        //         let err = err.unwrap_err();
        //         err
        //     })
        //     .collect();
        //let all_errs: Vec<u64> = errors.into_iter().map(|err| err.index()).collect();
        Err(CloudError::StateSyncError)
    }
}

pub fn parse_tx(
    tx: Transaction,
    eta: &Num<Fr>,
    params: &PoolParams
) -> Result<ParseResult, ParseError> {
    if tx.memo.len() < 4 {
        return Err(ParseError::NoPrefix(tx.index))
    }

    let (is_delegated_deposit, num_items) = parse_prefix(&tx.memo);
    // Special case: transaction contains delegated deposits
    if is_delegated_deposit {
        let num_deposits = num_items as usize;

        let delegated_deposits = tx.memo[4..]
            .chunks(MEMO_DELEGATED_DEPOSIT_SIZE)
            .take(num_deposits)
            .map(|data| MemoDelegatedDeposit::read(data))
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();

        let in_notes_indexed = delegated_deposits
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let p_d = derive_key_p_d(d.receiver_d.to_num(), eta.clone(), params).x;
                if d.receiver_p == p_d {
                    Some(IndexedNote {
                        index: tx.index + 1 + (i as u64),
                        note: d.to_delegated_deposit().to_note(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let in_notes: Vec<_> = in_notes_indexed.iter().map(|n| (n.index, n.note)).collect();

        let hashes = [zero_account().hash(params)]
            .iter()
            .copied()
            .chain(
                delegated_deposits
                    .iter()
                    .map(|d| d.to_delegated_deposit().to_note().hash(params)),
            )
            .collect();

        let parse_result = {
            if !in_notes.is_empty() {
                ParseResult {
                    decrypted_memos: vec![DecMemo {
                        index: tx.index,
                        in_notes: in_notes_indexed,
                        tx_hash: Some(tx.tx_hash),
                        ..Default::default()
                    }],
                    state_update: StateUpdate {
                        new_leafs: vec![(tx.index, hashes)],
                        new_notes: vec![in_notes],
                        ..Default::default()
                    },
                }
            } else {
                ParseResult {
                    state_update: StateUpdate {
                        new_commitments: vec![(tx.index, tx.commitment)],
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }
        };

        return Ok(parse_result);
    }

    // regular case: simple transaction memo
    let num_hashes = num_items;
    if num_hashes <= (constants::OUT + 1) as u32 {
        let hashes = (&tx.memo[4..])
            .chunks(32)
            .take(num_hashes as usize)
            .map(|bytes| Num::from_uint_reduced(NumRepr(Uint::from_little_endian(bytes))));
    
        let pair = cipher::decrypt_out(*eta, &tx.memo, params);

        match pair {
            Some((account, notes)) => {        
                let mut in_notes = Vec::new();
                let mut out_notes = Vec::new();
                notes.into_iter()
                    .enumerate()
                    .for_each(|(i, note)| {
                        out_notes.push((tx.index + 1 + (i as u64), note));

                        if note.p_d == derive_key_p_d(note.d.to_num(), *eta, params).x {
                            in_notes.push((tx.index + 1 + (i as u64), note));   
                        }
                    });

                Ok(ParseResult {
                    decrypted_memos: vec![ DecMemo {
                        index: tx.index,
                        acc: Some(account),
                        in_notes: in_notes.iter().map(|(index, note)| IndexedNote{index: *index, note: *note}).collect(), 
                        out_notes: out_notes.into_iter().map(|(index, note)| IndexedNote{index, note}).collect(), 
                        tx_hash: Some(tx.tx_hash),
                        ..Default::default()
                    }],
                    state_update: StateUpdate {
                        new_leafs: vec![(tx.index, hashes.collect())],
                        new_accounts: vec![(tx.index, account)],
                        new_notes: vec![in_notes],
                        ..Default::default()
                    }
                })
            },
            None => {
                let in_notes: Vec<(_, _)> = cipher::decrypt_in(*eta, &tx.memo, params)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, note)| {
                        match note {
                            Some(note) if note.p_d == derive_key_p_d(note.d.to_num(), *eta, params).x => {
                                Some((tx.index + 1 + (i as u64), note))
                            }
                            _ => None,
                        }
                    })
                    .collect();
                

                if !in_notes.is_empty() {
                    Ok(ParseResult {
                        decrypted_memos: vec![ DecMemo{
                            index: tx.index, 
                            in_notes: in_notes.iter().map(|(index, note)| IndexedNote{index: *index, note: *note}).collect(), 
                            tx_hash: Some(tx.tx_hash),
                            ..Default::default()
                        }],
                        state_update: StateUpdate {
                            new_leafs: vec![(tx.index, hashes.collect())],
                            new_notes: vec![in_notes],
                            ..Default::default()
                        }
                    })
                } else {
                    Ok(ParseResult {
                        state_update: StateUpdate {
                            new_commitments: vec![(tx.index, tx.commitment)],
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                }
            }
        }
    } else {
        Err(ParseError::IncorrectPrefix(tx.index, num_hashes, (constants::OUT + 1) as u32))
    }
}

fn parse_prefix(memo: &[u8]) -> (bool, u32) {
    let prefix = (&memo[0..4]).read_u32::<LittleEndian>().unwrap();
    let is_delegated_deposit = prefix & DELEGATED_DEPOSIT_FLAG > 0;
    match is_delegated_deposit {
        true => (true, (prefix ^ DELEGATED_DEPOSIT_FLAG)),
        false => (false, prefix)
    }
}