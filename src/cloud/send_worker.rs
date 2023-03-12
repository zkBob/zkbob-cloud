use std::{time::Duration, thread, str::FromStr};

use actix_web::web::Data;
use libzkbob_rs::proof::prove_tx;
use memo_parser::calldata::transact::memo::TxType;
use uuid::Uuid;
use zkbob_utils_rs::{tracing, relayer::types::{Proof, TransactionRequest}};

use crate::errors::CloudError;

use super::{cloud::ZkBobCloud, types::{TransferPart, TransferStatus}};

struct ProcessResult {
    delete: bool,
    check_status: Option<TransferPart>,
    update: Option<TransferPart>,
}

pub(crate) async fn run_send_worker(cloud: Data<ZkBobCloud>) -> Result<(), CloudError> {
    tokio::task::spawn(async move {
        loop {
            let mut send_queue = cloud.send_queue.write().await;
            match send_queue.receive::<TransferPart>().await {
                Ok(Some((redis_id, part))) => {
                    let process_result = process_part(cloud.clone(), part).await;
                    if process_result.delete {
                        send_queue.delete(&redis_id).await;
                    }

                },
                Ok(None) => {

                },
                Err(err) => {
                    tracing::error!("{}", err.to_string());
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
    Ok(())
}

async fn process_part(cloud: Data<ZkBobCloud>, part: TransferPart) -> ProcessResult {
    // 1. Check that task is new
    match part_status(cloud.clone(), &part.id).await {
        Ok(TransferStatus::New) => {
            // nothing to do
        },
        Ok(status) => {
            return ProcessResult {
                delete: true,
                check_status: None,
                update: None,
            }
        },
        Err(err) => {
            let part = TransferPart {
                status: TransferStatus::Failed(err),
                ..part
            };
            return ProcessResult {
                delete: true,
                check_status: None,
                update: Some(part),
            };
        }
    }
    
    // 2. Check that previous task have already been accepted by relayer
    if part.depends_on.is_some() {
        match part_status(cloud.clone(), part.depends_on.as_ref().unwrap()).await {
            Ok(TransferStatus::Failed(err)) => {
                let part = TransferPart {
                    status: TransferStatus::Failed(CloudError::PreviousTxFailed),
                    ..part
                };
                return ProcessResult {
                    delete: true,
                    check_status: None,
                    update: Some(part),
                };
            },
            Ok(TransferStatus::Relaying | TransferStatus::Mining | TransferStatus::Done) => {
                // nothing to do
            },
            Ok(status) => {
                return ProcessResult {
                    delete: false,
                    check_status: None,
                    update: None,
                };
            },
            Err(err) => {
                let part = TransferPart {
                    status: TransferStatus::Failed(err),
                    ..part
                };
                return ProcessResult {
                    delete: true,
                    check_status: None,
                    update: Some(part),
                };
            }
        }
    }

    // 3. Get account
    let account_id = Uuid::from_str(&part.account_id);
    if let Err(err) = account_id {
        let part = TransferPart {
            status: TransferStatus::Failed(CloudError::IncorrectAccountId),
            ..part
        };
        return ProcessResult {
            delete: true,
            check_status: None,
            update: Some(part),
        };
    }
    let account_id = account_id.unwrap();

    let tx = {
        let account = cloud.get_account(account_id).await;
        if let Err(err) = account {
            let part = TransferPart {
                status: TransferStatus::Failed(err),
                ..part
            };
            return ProcessResult {
                delete: true, // TODO: add attempts
                check_status: None,
                update: Some(part),
            };
        }
        let account = account.unwrap();
        
        // Prepare tx with optimistic state
        let tx = account.create_transfer(part.amount, part.to.clone(), part.fee, cloud.relayer.clone()).await;
        if let Err(err) = tx {
            let part = TransferPart {
                status: TransferStatus::Failed(err),
                ..part
            };
            return ProcessResult {
                delete: true, // TODO: add attempts
                check_status: None,
                update: Some(part),
            };
        }
        cloud.release_account(account_id).await;
    
        tx.unwrap()
    };
    
    // Prove tx
    let proving_span = tracing::info_span!("proving", request_id = part.request_id.clone());
    let (inputs, proof) = proving_span.in_scope(|| {
        prove_tx(
            &cloud.params,
            &*libzkbob_rs::libzeropool::POOL_PARAMS,
            tx.public,
            tx.secret,
        )
    });

    // 5. Send tx to relayer
    let proof = Proof { inputs, proof };
    let request = vec![TransactionRequest {
        uuid: Some(Uuid::new_v4().to_string()),
        proof,
        memo: hex::encode(tx.memo),
        tx_type: format!("{:0>4}", TxType::Transfer.to_u32()),
        deposit_signature: None,
    }];
    let response = cloud.relayer.send_transactions(request).await;
    println!("{:?}", response);
    if let Err(err) = response {
        let part = TransferPart {
            status: TransferStatus::Failed(err),
            ..part
        };
        return ProcessResult {
            delete: true, // TODO: add attempts
            check_status: None,
            update: Some(part),
        };
    }
    let response = response.unwrap();

    let part = TransferPart {
        status: TransferStatus::Relaying,
        job_id: Some(response.job_id),
        ..part
    };

    return ProcessResult {
        delete: true,
        check_status: Some(part.clone()),
        update: Some(part),
    };
}

async fn part_status(cloud: Data<ZkBobCloud>, part_id: &str) -> Result<TransferStatus, CloudError> {
    let db = cloud.db.read().await;
    let part = db.get_part(part_id)?;
    Ok(part.status)
}