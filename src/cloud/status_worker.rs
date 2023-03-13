use std::{thread, time::Duration};

use actix_web::web::Data;
use zkbob_utils_rs::{tracing, relayer::types::JobResponse};

use crate::{errors::CloudError, cloud::{send_worker::get_part, types::TransferStatus}};

use super::{cloud::ZkBobCloud, types::TransferPart};

const MAX_ATTEMPTS: u32 = 5;

pub(crate) async fn run_status_worker(cloud: Data<ZkBobCloud>) -> Result<(), CloudError> {
    tokio::task::spawn(async move {
        loop {
            let task = {
                let mut status_queue = cloud.status_queue.write().await;
                status_queue.receive::<String>().await
            };
            match task {
                Ok(Some((redis_id, id))) => {
                    println!("status: {}", &id);
                    let process_result = process_part(cloud.clone(), id.clone()).await;
                    println!("status: {:?}", &process_result);
                    if process_result.update.is_some() {
                        match cloud.db.write().await.save_part(&process_result.update.unwrap()) {
                            Ok(_) => {},
                            Err(err) => {
                                continue;
                            }
                        }
                    }
                    
                    if process_result.delete {
                        let mut status_queue = cloud.status_queue.write().await;
                        status_queue.delete(&redis_id).await;
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

async fn process_part(cloud: Data<ZkBobCloud>, id: String) -> ProcessResult {
    // 1. Check that task is relaying
    let part = get_part(cloud.clone(), &id).await;
    if let Err(err) = part {
        return ProcessResult {
            delete: true,
            update: None,
        }
    }
    let part = part.unwrap();

    match part.status.clone() {
        TransferStatus::Relaying => {},
        status => {
            return ProcessResult {
                delete: true,
                update: None,
            }
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
                    let part = TransferPart {
                        status,
                        tx_hash: response.tx_hash,
                        ..part
                    };
                    ProcessResult {
                        delete: true,
                        update: Some(part)
                    }
                }
                TransferStatus::Failed(err) => {
                    ProcessResult::error_without_retry(part, err)
                },
                _ => ProcessResult::retry_later()
            }
        },
        Err(err) => ProcessResult::error_with_retry_attempts(part, err)
    }
}


#[derive(Debug)]
struct ProcessResult {
    delete: bool,
    update: Option<TransferPart>,
}

impl ProcessResult {
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

    fn retry_later() -> ProcessResult {
        return ProcessResult {
            delete: false,
            update: None,
        };
    }
}