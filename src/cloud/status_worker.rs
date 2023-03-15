use std::{thread, time::Duration, str::FromStr};

use actix_web::web::Data;
use tokio::time;
use uuid::Uuid;
use zkbob_utils_rs::{tracing, relayer::types::JobResponse};

use crate::{errors::CloudError, cloud::{send_worker::get_part, types::TransferStatus}, helpers::timestamp};

use super::{cloud::ZkBobCloud, types::TransferPart};

const MAX_ATTEMPTS: u32 = 10;

pub(crate) async fn run_status_worker(cloud: Data<ZkBobCloud>) -> Result<(), CloudError> {
    tokio::task::spawn(async move {
        loop {
            let task = {
                let mut status_queue = cloud.status_queue.write().await;
                status_queue.receive::<String>().await
            };
            match task {
                Ok(Some((redis_id, id))) => {
                    let cloud = cloud.clone();
                    thread::spawn(move || {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        rt.block_on(async {
                            let process_result = process(cloud.clone(), id.clone()).await;
                            if let Err(_) = postprocessing(cloud.clone(), &process_result).await {
                                return;
                            }
                            
                            if process_result.delete {
                                let mut status_queue = cloud.status_queue.write().await;
                                if let Err(err) = status_queue.delete(&redis_id).await {
                                    tracing::error!("[status task: {}] failed to delete task from queue: {}", &id, err);
                                    return;
                                }
                            }
                        })
                    });
                },
                Ok(None) => {
                    time::sleep(Duration::from_millis(500)).await;
                },
                Err(_) => {
                    let mut status_queue = cloud.status_queue.write().await;
                    match status_queue.reconnect().await {
                        Ok(_) => tracing::info!("connection to redis reestablished"),
                        Err(_) => {
                            time::sleep(Duration::from_millis(5000)).await;
                        }
                    }
                }
            }
            time::sleep(Duration::from_millis(500)).await;
        }
    });
    Ok(())
}

async fn process(cloud: Data<ZkBobCloud>, id: String) -> ProcessResult {
    tracing::info!("[status task: {}] processing...", &id);

    let part = match get_part(cloud.clone(), &id).await {
        Ok(part) => part,
        Err(err) => {
            tracing::error!("[status task: {}] cannot get task from db: {}, deleting task", &id, err);
            return ProcessResult::delete_from_queue();
        }
    };

    match part.status.clone() {
        TransferStatus::Relaying | TransferStatus::Mining => {},
        status => {
            tracing::warn!("[status task: {}] task has status {:?}, deleting task", &id, status);
            return ProcessResult::delete_from_queue();
        }
    }

    let response: Result<JobResponse, CloudError> = cloud.relayer.job(&part.job_id.clone().unwrap()).await;
    match response {
        Ok(response) => {
            let status = TransferStatus::from_relayer_response(
                response.state,
                response.failed_reason,
            );

            match status {
                TransferStatus::Done => {
                    tracing::info!("[status task: {}] processed successfully, tx_hash: {}", &id, &response.tx_hash.clone().unwrap());
                    ProcessResult::success(part, response.tx_hash.unwrap())
                }
                TransferStatus::Mining => {
                    tracing::info!("[status task: {}] sent to contract, tx_hash: {}", &id, &response.tx_hash.clone().unwrap());
                    ProcessResult::update_status(part, TransferStatus::Mining, response.tx_hash.unwrap())
                }
                TransferStatus::Failed(err) => {
                    tracing::warn!("[status task: {}] task was rejected by relayer: {}", &id, err);
                    ProcessResult::rejected(part, err, response.tx_hash)
                },
                _ => {
                    tracing::info!("[status task: {}] task is not finished yet, postpone task", &id);
                    ProcessResult::retry_later()
                }
            }
        },
        Err(err) => {
            tracing::warn!("[status task: {}] failed to fetch status from relayer, retry attempt: {}", &id, part.attempt);
            ProcessResult::error_with_retry_attempts(part, err)
        }
    }
}

async fn postprocessing(cloud: Data<ZkBobCloud>, process_result: &ProcessResult) -> Result<(), ()> {
    let part = match process_result.part.clone() {
        Some(part) => part,
        None => {
            return Ok(())
        }
    };

    if process_result.update {
        if let Err(err) = cloud.db.write().await.save_part(&part) {
            tracing::error!("[status task: {}] failed to save processed task in db: {}", &part.id, err);
            return Err(());
        }
    }

    // it is not critical
    if process_result.save_transaction_id {
        let account_id = match Uuid::from_str(&part.account_id) {
            Ok(id) => id,
            Err(err) => {
                tracing::warn!("[status task: {}] failed to parse account_id: {}", &part.id, err);
                return Ok(());
            }
        };

        let (account, _cleanup) =  match cloud.get_account(account_id).await {
            Ok((account, cleanup)) => (account, cleanup),
            Err(err) => {
                tracing::warn!("[status task: {}] failed to get account: {}", &part.id, err);
                return Ok(());
            }
        };

        if let Err(err) = account.save_transaction_id(&part.tx_hash.clone().unwrap(), &part.request_id).await {
            tracing::warn!("[status task: {}] failed to save transaction id: {}", &part.id, err);
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
        return ProcessResult {
            part: None,
            delete: false,
            update: false,
            save_transaction_id: false,
        };
    }

    fn delete_from_queue() -> ProcessResult {
        return ProcessResult {
            part: None,
            delete: true,
            update: false,
            save_transaction_id: false,
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
            part: Some(part),
            delete: false,
            update: true,
            save_transaction_id: false,
        };
    }

    fn error_without_retry(part: TransferPart, err: CloudError) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Failed(err),
            timestamp: timestamp(),
            ..part
        };
        return ProcessResult {
            part: Some(part),
            delete: true,
            update: true,
            save_transaction_id: false,
        };
    }
}