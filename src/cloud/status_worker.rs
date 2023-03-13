use std::{thread, time::Duration};

use actix_web::web::Data;
use tokio::time;
use zkbob_utils_rs::{tracing, relayer::types::JobResponse};

use crate::{errors::CloudError, cloud::{send_worker::get_part, types::TransferStatus}};

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
                            let process_result = process_part(cloud.clone(), id.clone()).await;
                            if process_result.update.is_some() {
                                if let Err(err) = cloud.db.write().await.save_part(&process_result.update.unwrap()) {
                                    tracing::error!("[status task: {}] failed to save processed task in db: {}", &id, err);
                                    return;
                                }
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
                Ok(None) => { },
                Err(err) => {
                    tracing::error!("failed to recieve task from status queue: {}", err.to_string());
                }
            }
            time::sleep(Duration::from_millis(500)).await;
        }
    });
    Ok(())
}

async fn process_part(cloud: Data<ZkBobCloud>, id: String) -> ProcessResult {
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
                    ProcessResult::error_without_retry(part, err)
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


#[derive(Debug)]
struct ProcessResult {
    delete: bool,
    update: Option<TransferPart>,
}

impl ProcessResult {
    fn success(part: TransferPart, tx_hash: String) -> ProcessResult {
        let part = TransferPart {
            status: TransferStatus::Done,
            tx_hash: Some(tx_hash),
            ..part
        };
        ProcessResult {
            delete: true,
            update: Some(part)
        }
    }

    fn update_status(part: TransferPart, status: TransferStatus, tx_hash: String) -> ProcessResult {
        let part = TransferPart {
            status,
            tx_hash: Some(tx_hash),
            ..part
        };
        ProcessResult {
            delete: false,
            update: Some(part)
        }
    }

    fn retry_later() -> ProcessResult {
        return ProcessResult {
            delete: false,
            update: None,
        };
    }

    fn delete_from_queue() -> ProcessResult {
        return ProcessResult {
            delete: true,
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
            update: Some(part),
        };
    }
}