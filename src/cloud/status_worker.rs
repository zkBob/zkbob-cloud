use std::{thread, sync::Arc, collections::HashSet};

use actix_web::web::Data;
use tokio::{sync::RwLock};
use zkbob_utils_rs::{tracing, relayer::types::JobResponse};

use crate::{errors::CloudError, cloud::{send_worker::get_part, types::TransferStatus}, helpers::{timestamp, queue::receive_blocking}};

use super::{ZkBobCloud, types::TransferPart, cleanup::WorkerCleanup};

pub(crate) fn run_status_worker(cloud: Data<ZkBobCloud>, max_attempts: u32) {
    thread::spawn( move || {
        let _cleanup = WorkerCleanup;
        let rt = tokio::runtime::Runtime::new().expect("failed to init tokio runtime");
        rt.block_on(async move {
            let in_progress = Arc::new(RwLock::new(HashSet::new()));
            loop {
                let (redis_id, id) = receive_blocking::<String>(cloud.status_queue.clone()).await;

                if !in_progress.write().await.insert(redis_id.clone()) {
                    continue;
                }

                let in_progress = in_progress.clone();
                let cloud = cloud.clone();
                tokio::spawn(async move {
                    let process_result = process(&cloud, &id, max_attempts).await;
                    if postprocessing(&cloud, &process_result).await.is_err() {
                        in_progress.write().await.remove(&redis_id);
                        return;
                    }
                    
                    if process_result.delete {
                        let mut status_queue = cloud.status_queue.write().await;
                        if let Err(err) = status_queue.delete(&redis_id).await {
                            tracing::error!("[status task: {}] failed to delete task from queue: {}", &id, err);
                            in_progress.write().await.remove(&redis_id);
                            return;
                        }
                    }

                    in_progress.write().await.remove(&redis_id);
                });
            }
        });
    });
}

async fn process(cloud: &ZkBobCloud, id: &str, max_attempts: u32) -> ProcessResult {
    tracing::info!("[status task: {}] processing...", id);

    let part = match get_part(cloud, id).await {
        Ok(part) => part,
        Err(err) => {
            tracing::error!("[status task: {}] cannot get task from db: {}, deleting task", id, err);
            return ProcessResult::delete_from_queue();
        }
    };

    match &part.status {
        TransferStatus::Relaying | TransferStatus::Mining => {},
        status => {
            tracing::warn!("[status task: {}] task has status {:?}, deleting task", id, status);
            return ProcessResult::delete_from_queue();
        }
    }

    let job_id = match part.job_id.as_ref() {
        Some(job_id) => job_id,
        None => {
            tracing::error!("[status task: {}] task has status {:?} but doesn't contain job id, deleting task", id, part.status);
            return ProcessResult::delete_from_queue();
        }
    };

    let response: Result<JobResponse, CloudError> = cloud.relayer.job(job_id).await;
    match response {
        Ok(response) => {
            let status = TransferStatus::from_relayer_response(
                response.state,
                response.failed_reason,
            );

            match status {
                TransferStatus::Done => {
                    let tx_hash = match response.tx_hash {
                        Some(tx_hash) => tx_hash,
                        None => {
                            tracing::info!("[status task: {}] transfer status is done but tx hash is not found", id);
                            return ProcessResult::error_with_retry_attempts(part, CloudError::RelayerSendError, max_attempts);
                        }
                    };
                    tracing::info!("[status task: {}] processed successfully, tx_hash: {}", id, &tx_hash);
                    ProcessResult::success(part, tx_hash)
                }
                TransferStatus::Mining => {
                    let tx_hash = match response.tx_hash {
                        Some(tx_hash) => tx_hash,
                        None => {
                            tracing::info!("[status task: {}] transfer status is done but tx hash is not found", id);
                            return ProcessResult::error_with_retry_attempts(part, CloudError::RelayerSendError, max_attempts);
                        }
                    };
                    tracing::info!("[status task: {}] sent to contract, tx_hash: {}", id, &tx_hash);
                    ProcessResult::update_status(part, TransferStatus::Mining, tx_hash)
                }
                TransferStatus::Failed(err) => {
                    tracing::warn!("[status task: {}] task was rejected by relayer: {}", id, err);
                    ProcessResult::rejected(part, err, response.tx_hash)
                },
                _ => {
                    tracing::info!("[status task: {}] task is not finished yet, postpone task", id);
                    ProcessResult::retry_later()
                }
            }
        },
        Err(err) => {
            tracing::warn!("[status task: {}] failed to fetch status from relayer, retry attempt: {}", id, part.attempt);
            ProcessResult::error_with_retry_attempts(part, err, max_attempts)
        }
    }
}

async fn postprocessing(cloud: &ZkBobCloud, process_result: &ProcessResult) -> Result<(), ()> {
    let part = match &process_result.part {
        Some(part) => part,
        None => {
            return Ok(())
        }
    };

    if process_result.update {
        if let Err(err) = cloud.db.write().await.save_part(part) {
            tracing::error!("[status task: {}] failed to save processed task in db: {}", &part.id, err);
            return Err(());
        }
    }

    // it is not critical
    if process_result.save_transaction_id {
        if let Some(tx_hash) = &part.tx_hash {
            if let Err(err) = cloud.db.write().await.save_transaction_id(tx_hash, &part.request_id) {
                tracing::warn!("[status task: {}] failed to save transaction id: {}", &part.id, err);
            }
        }
    }
    Ok(())
}


#[derive(Debug)]
struct ProcessResult {
    part: Option<TransferPart>,
    delete: bool,
    update: bool,
    save_transaction_id: bool,
}

impl ProcessResult {
    fn success(part: TransferPart, tx_hash: String) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Done,
            tx_hash: Some(tx_hash),
            timestamp: timestamp(),
            ..part
        };
        ProcessResult {
            part: Some(part),
            delete: true,
            update: true,
            save_transaction_id: true,
        }
    }

    fn rejected(part: TransferPart, err: CloudError, tx_hash: Option<String>) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Failed(err),
            tx_hash,
            timestamp: timestamp(),
            ..part
        };
        ProcessResult {
            part: Some(part),
            delete: true,
            update: true,
            save_transaction_id: false,
        }
    }

    fn update_status(part: TransferPart, status: TransferStatus, tx_hash: String) -> ProcessResult {
        let part = TransferPart {
            status,
            tx_hash: Some(tx_hash),
            ..part
        };
        ProcessResult {
            part: Some(part),
            delete: false,
            update: true,
            save_transaction_id: false,
        }
    }

    fn retry_later() -> ProcessResult {
        ProcessResult {
            part: None,
            delete: false,
            update: false,
            save_transaction_id: false,
        }
    }

    fn delete_from_queue() -> ProcessResult {
        ProcessResult {
            part: None,
            delete: true,
            update: false,
            save_transaction_id: false,
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
            part: Some(part),
            delete: false,
            update: true,
            save_transaction_id: false,
        }
    }

    fn error_without_retry(part: TransferPart, err: CloudError) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Failed(err),
            timestamp: timestamp(),
            ..part
        };
        ProcessResult {
            part: Some(part),
            delete: true,
            update: true,
            save_transaction_id: false,
        }
    }
}