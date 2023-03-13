use std::{time::Duration, thread, str::FromStr, sync::Arc};

use actix_web::web::Data;
use libzkbob_rs::proof::prove_tx;
use memo_parser::calldata::transact::memo::TxType;
use tokio::sync::RwLock;
use uuid::Uuid;
use zkbob_utils_rs::{tracing, relayer::types::{Proof, TransactionRequest}};

use crate::errors::CloudError;

use super::{cloud::ZkBobCloud, types::{TransferPart, TransferStatus}, queue::Queue};

const MAX_ATTEMPTS: u32 = 5;

pub(crate) async fn run_send_worker(cloud: Data<ZkBobCloud>, check_status_queue: Arc<RwLock<Queue>>) -> Result<(), CloudError> {
    tokio::task::spawn(async move {
        loop {
            let task = {
                let mut send_queue = cloud.send_queue.write().await;
                send_queue.receive::<String>().await
            };
            match task {
                Ok(Some((redis_id, id))) => {
                    let cloud = cloud.clone();
                    let check_status_queue = check_status_queue.clone();
                    tokio::task::spawn(async move {
                        println!("status: {:?}", &id);
                        let process_result = process_part(cloud.clone(), id.clone()).await;
                        println!("status: {:?}", &process_result);
                        if process_result.update.is_some() {
                            match cloud.db.write().await.save_part(&process_result.update.unwrap()) {
                                Ok(_) => {},
                                Err(err) => {
                                    return;
                                }
                            }
                        }

                        if process_result.check_status {
                            match check_status_queue.write().await.send(id).await {
                                Ok(_) => {},
                                Err(err) => {
                                    return;
                                }
                            }
                        }
                        
                        if process_result.delete {
                            let mut send_queue = cloud.send_queue.write().await;
                            send_queue.delete(&redis_id).await;
                        }
                    });
                },
                Ok(None) => { },
                Err(err) => {
                    tracing::error!("{}", err.to_string());
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
    Ok(())
}

async fn process_part(cloud: Data<ZkBobCloud>, id: String) -> ProcessResult {
    // 1. Check that task is new
    let part = get_part(cloud.clone(), &id).await;
    if let Err(err) = part {
        return ProcessResult {
            delete: true,
            check_status: false,
            update: None,
        }
    }
    let part = part.unwrap();

    match part.status.clone() {
        TransferStatus::New => {
            // nothing to do
        },
        TransferStatus::Relaying => {
            return ProcessResult {
                delete: true,
                check_status: true,
                update: None,
            }
        },
        status => {
            return ProcessResult {
                delete: true,
                check_status: false,
                update: None,
            }
        }
    }
    
    // 2. Check that previous task have already been accepted by relayer
    if part.depends_on.is_some() {
        match part_status(cloud.clone(), part.depends_on.as_ref().unwrap()).await {
            Ok(TransferStatus::Relaying | TransferStatus::Mining | TransferStatus::Done) => {
                // nothing to do
            },
            Ok(TransferStatus::Failed(err)) => {
                return ProcessResult::error_without_retry(part, CloudError::PreviousTxFailed)
            },
            Ok(status) => {
                return ProcessResult::retry_later();
            },
            Err(err) => {
                return ProcessResult::error_with_retry_attempts(part, err);
            }
        }
    }

    // 3. Get account
    let account_id = Uuid::from_str(&part.account_id);
    if let Err(err) = account_id {
        return ProcessResult::error_without_retry(part, CloudError::IncorrectAccountId);
    }
    let account_id = account_id.unwrap();

    let tx = {
        let account = cloud.get_account(account_id).await;
        if let Err(err) = account {
            return ProcessResult::error_with_retry_attempts(part, err);
        }
        let account = account.unwrap();
        
        // Prepare tx with optimistic state
        let tx = account.create_transfer(part.amount, part.to.clone(), part.fee, cloud.relayer.clone()).await;
        if let Err(err) = tx {
            return ProcessResult::error_with_retry_attempts(part, err);
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
    
    if let Err(err) = response {
        return ProcessResult::error_with_retry_attempts(part, err);
    }
    let response = response.unwrap();

    let part = TransferPart {
        status: TransferStatus::Relaying,
        job_id: Some(response.job_id),
        attempt: 0,
        ..part
    };

    return ProcessResult {
        delete: true,
        check_status: true,
        update: Some(part),
    };
}

#[derive(Debug)]
struct ProcessResult {
    delete: bool,
    check_status: bool,
    update: Option<TransferPart>,
}

impl ProcessResult {
    fn retry_later() -> ProcessResult {
        return ProcessResult {
            delete: false,
            check_status: false,
            update: None,
        };
    }

    fn error_with_retry_attempts(part: TransferPart, err: CloudError) -> ProcessResult {
        if part.attempt >= MAX_ATTEMPTS {
            return ProcessResult::error_without_retry(part, err);
        }

        let part = TransferPart {
            attempt: part.attempt + 1,
            ..part
        };
        return ProcessResult {
            delete: false,
            check_status: false,
            update: Some(part),
        };
    }

    fn error_without_retry(part: TransferPart, err: CloudError) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Failed(err),
            ..part
        };
        return ProcessResult {
            delete: true,
            check_status: false,
            update: Some(part),
        };
    }
}


pub(crate) async fn get_part(cloud: Data<ZkBobCloud>, part_id: &str) -> Result<TransferPart, CloudError> {
    let db = cloud.db.read().await;
    let part = db.get_part(part_id)?;
    Ok(part)
}

pub(crate) async fn part_status(cloud: Data<ZkBobCloud>, part_id: &str) -> Result<TransferStatus, CloudError> {
    let part = get_part(cloud, part_id).await?;
    Ok(part.status)
}