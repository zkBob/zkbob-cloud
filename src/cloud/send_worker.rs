use std::{time::Duration, thread, str::FromStr, sync::Arc, collections::HashSet};

use actix_web::web::Data;
use libzkbob_rs::proof::prove_tx;
use memo_parser::calldata::transact::memo::TxType;
use tokio::{sync::RwLock, time, task};
use uuid::Uuid;
use zkbob_utils_rs::{tracing, relayer::types::{Proof, TransactionRequest}};

use crate::{errors::CloudError, helpers::timestamp};

use super::{ZkBobCloud, types::{TransferPart, TransferStatus}};

pub(crate) fn run_send_worker(cloud: Data<ZkBobCloud>, max_attempts: u32) {
    thread::spawn( move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let in_progress = Arc::new(RwLock::new(HashSet::new()));
            loop {
                let task = {
                    let mut send_queue = cloud.send_queue.write().await;
                    send_queue.receive::<String>().await
                };
                match task {
                    Ok(Some((redis_id, id))) => {
                        if !in_progress.write().await.insert(redis_id.clone()) {
                            continue;
                        }
                        let cloud = cloud.clone();
                        let in_progress = in_progress.clone();
                        tokio::spawn(async move {
                            let process_result = process(&cloud, &id, max_attempts).await;
                            if let Some(update) = process_result.update {
                                if let Err(err) = cloud.db.write().await.save_part(&update) {
                                    tracing::error!("[send task: {}] failed to save processed task in db: {}", &id, err);
                                    in_progress.write().await.remove(&redis_id);
                                    return;
                                }
                            }
    
                            if process_result.check_status {
                                if let Err(err) = cloud.status_queue.write().await.send(id.clone()).await {
                                    tracing::error!("[send task: {}] failed to send task to check status queue: {}", &id, err);
                                    in_progress.write().await.remove(&redis_id);
                                    return;
                                }
                            }
                            
                            if process_result.delete {
                                let mut send_queue = cloud.send_queue.write().await;
                                if let Err(err) = send_queue.delete(&redis_id).await {
                                    tracing::error!("[send task: {}] failed to delete task from queue: {}", &id, err);
                                    in_progress.write().await.remove(&redis_id);
                                    return;
                                }
                            }
    
                            in_progress.write().await.remove(&redis_id);
                        });
                    },
                    Ok(None) => {
                        time::sleep(Duration::from_millis(500)).await;
                    },
                    Err(_) => {
                        let mut send_queue = cloud.send_queue.write().await;
                        match send_queue.reconnect().await {
                            Ok(_) => tracing::info!("connection to redis reestablished"),
                            Err(_) => {
                                time::sleep(Duration::from_millis(5000)).await;
                            }
                        }
                    }
                }
            }
        })        
    });
}

async fn process(cloud: &ZkBobCloud, id: &str, max_attempts: u32) -> ProcessResult {
    let part = match get_part(cloud, id).await {
        Ok(part) => part,
        Err(err) => {
            tracing::error!("[send task: {}] cannot get task from db: {}, deleting task", id, err);
            return ProcessResult::delete_from_queue();
        }
    };

    match &part.status {
        TransferStatus::New => {},
        TransferStatus::Relaying | TransferStatus::Mining => {
            tracing::warn!("[send task: {}] task has status Relaying or Mining, trying to initiate check status again", id);
            return ProcessResult::repeat_check_status();
        }
        status => {
            tracing::warn!("[send task: {}] task has status {:?}, deleting task", id, status);
            return ProcessResult::delete_from_queue();
        }
    }
    
    if let Some(depends_on) = part.depends_on.as_ref() {
        match part_status(cloud, depends_on).await {
            Ok(TransferStatus::Mining | TransferStatus::Done) => { },
            Ok(TransferStatus::Failed(_)) => {
                tracing::warn!("[send task: {}] previous task has failed, marking task as failed", id);
                return ProcessResult::error_without_retry(part, CloudError::PreviousTxFailed)
            },
            Ok(status) => {
                tracing::debug!("[send task: {}] previous task has status {:?}, postpone task", id, status);
                return ProcessResult::retry_later();
            },
            Err(err) => {
                tracing::warn!("[send task: {}] failed to get status of previous task, retry attempt: {}", id, part.attempt);
                return ProcessResult::error_with_retry_attempts(part, err, max_attempts);
            }
        }
    }

    tracing::info!("[send task: {}] processing...", id);

    let account_id = match Uuid::from_str(&part.account_id) {
        Ok(account_id) => account_id,
        Err(_) => {
            tracing::error!("[send task: {}] failed to parse account id: {}, marking task as failed", id, &part.account_id);
            return ProcessResult::error_without_retry(part, CloudError::IncorrectAccountId);
        }
    };

    let tx = {  
        let (account, _cleanup) = match cloud.get_account(account_id).await {
            Ok(account) => account,
            Err(err) => {
                tracing::warn!("[send task: {}] failed to get account, retry attempt: {}", id, part.attempt);
                return ProcessResult::error_with_retry_attempts(part, err, max_attempts);
            }
        };
        
        let tx = match account.create_transfer(part.amount, part.to.clone(), part.fee, &cloud.relayer).await {
            Ok(tx) => tx,
            Err(err) => {
                tracing::warn!("[send task: {}] failed to create transfer, retry attempt: {}", id, part.attempt);
                return ProcessResult::error_with_retry_attempts(part, err, max_attempts);
            }
        };  
        tx
    };
    
    let prove_result = {
        let params = cloud.params.clone();
        let proving_span = tracing::info_span!("proving", task_id = &part.id);
        task::spawn_blocking(move || {
            proving_span.in_scope(|| {
                prove_tx(
                    &params,
                    &*libzkbob_rs::libzeropool::POOL_PARAMS,
                    tx.public,
                    tx.secret,
                )
            })
        }).await
    };

    let (inputs, proof) = match prove_result {
        Ok((inputs, proof)) => (inputs, proof),
        Err(err) => {
            tracing::warn!("[send task: {}] failed to prove transfer: {}, retry attempt: {}", id, err, part.attempt);
            return ProcessResult::error_with_retry_attempts(part, CloudError::InternalError("prove error".to_string()), max_attempts);
        }
    };

    let proof = Proof { inputs, proof };
    let request = vec![TransactionRequest {
        uuid: Some(Uuid::new_v4().to_string()),
        proof,
        memo: hex::encode(tx.memo),
        tx_type: format!("{:0>4}", TxType::Transfer.to_u32()),
        deposit_signature: None,
    }];

    let response = match cloud.relayer.send_transactions(request).await {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!("[send task: {}] failed send transfer to relayer, retry attempt: {}", id, part.attempt);
            return ProcessResult::error_with_retry_attempts(part, err, max_attempts);
        }
    };

    tracing::info!("[send task: {}] processed successfully, job_id: {}", id, &response.job_id);
    ProcessResult::success(part, response.job_id)    
}

#[derive(Debug)]
struct ProcessResult {
    delete: bool,
    check_status: bool,
    update: Option<TransferPart>,
}

impl ProcessResult {
    fn success(part: TransferPart, job_id: String) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Relaying,
            job_id: Some(job_id),
            attempt: 0,
            timestamp: timestamp(),
            ..part
        };
    
        ProcessResult {
            delete: true,
            check_status: true,
            update: Some(part),
        }
    }

    fn retry_later() -> ProcessResult {
        ProcessResult {
            delete: false,
            check_status: false,
            update: None,
        }
    }

    fn delete_from_queue() -> ProcessResult {
        ProcessResult {
            delete: true,
            check_status: false,
            update: None,
        }
    }

    fn repeat_check_status() -> ProcessResult {
        ProcessResult {
            delete: true,
            check_status: true,
            update: None,
        }
    }

    fn error_with_retry_attempts(part: TransferPart, err: CloudError, max_attempts: u32) -> ProcessResult {
        if part.attempt >= max_attempts {
            return ProcessResult::error_without_retry(part, err);
        }

        let part = TransferPart {
            attempt: part.attempt + 1,
            ..part
        };
        ProcessResult {
            delete: false,
            check_status: false,
            update: Some(part),
        }
    }

    fn error_without_retry(part: TransferPart, err: CloudError) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Failed(err),
            timestamp: timestamp(),
            ..part
        };
        ProcessResult {
            delete: true,
            check_status: false,
            update: Some(part),
        }
    }
}


pub(crate) async fn get_part(cloud: &ZkBobCloud, part_id: &str) -> Result<TransferPart, CloudError> {
    let db = cloud.db.read().await;
    let part = db.get_part(part_id)?;
    Ok(part)
}

pub(crate) async fn part_status(cloud: &ZkBobCloud, part_id: &str) -> Result<TransferStatus, CloudError> {
    let part = get_part(cloud, part_id).await?;
    Ok(part.status)
}