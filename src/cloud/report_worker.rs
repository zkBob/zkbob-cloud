use std::{thread, str::FromStr};

use actix_web::web::Data;
use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::{cloud::types::AccountReport, helpers::{timestamp, queue::receive_blocking}};

use super::{cleanup::WorkerCleanup, ZkBobCloud, types::{ReportTask, ReportStatus, Report}};


pub(crate) fn run_report_worker(cloud: Data<ZkBobCloud>, max_attempts: u32) {
    thread::spawn( move || {
        let _cleanup = WorkerCleanup;
        let rt = tokio::runtime::Runtime::new().expect("failed to init tokio runtime");
        rt.block_on(async move {
            loop {
                let (redis_id, id) = receive_blocking::<String>(cloud.report_queue.clone()).await;

                let process_result = process(&cloud, &id, max_attempts).await;
                if let Some(update) = process_result.update {
                    if let Err(err) = cloud.db.write().await.save_report_task(Uuid::from_str(&id).unwrap(), &update) {
                        tracing::error!("[report task: {}] failed to save processed task in db: {}", &id, err);
                        continue;
                    }

                    if process_result.delete {
                        let mut report_queue = cloud.report_queue.write().await;
                        if let Err(err) = report_queue.delete(&redis_id).await {
                            tracing::error!("[report task: {}] failed to delete task from queue: {}", &id, err);
                            continue;
                        }
                    }
                }
            }
        });
    });
}

async fn process(cloud: &ZkBobCloud, id: &str, max_attempts: u32) -> ProcessResult {
    let id = match Uuid::from_str(id) {
        Ok(id) => id,
        Err(err) => {
            tracing::warn!("[report task: {}] failed to parse report id: {}", id, err);
            return ProcessResult::delete_from_queue();
        }
    };
    
    let task = match cloud.db.read().await.get_report_task(id) {
        Ok(Some(task)) => task,
        _ => {
            tracing::error!("[report task: {}] failed to get from db", id);
            return ProcessResult::delete_from_queue();
        }
    };

    tracing::info!("[report task: {}] processing...", id);

    let accounts = match cloud.db.read().await.get_accounts() {
        Ok(accounts) => accounts,
        Err(err) => {
            tracing::warn!("[report task: {}] failed to get accounts from db, attempt: {}. Error: {}", id, task.attempt, err);
            return ProcessResult::error_with_retry_attempts(task, max_attempts);
        }
    };

    let to_index = match cloud.relayer.info().await {
        Ok(info) => info.delta_index,
        Err(err) => {
            tracing::warn!("[report task: {}] failed to fetch info from relayer, attempt: {}. Error: {}", id, task.attempt, err);
            return ProcessResult::error_with_retry_attempts(task, max_attempts);
        }
    };

    let mut reports = vec![];
    let count = accounts.len();
    for (i, (account_id, _)) in accounts.into_iter().enumerate() {
        let (account, _cleanup) = match cloud.get_account(account_id).await {
            Ok((account, cleanup)) => (account, cleanup),
            Err(err) => {
                tracing::warn!("[report task: {}] failed to get account {}, attempt: {}. Error: {}", id, account_id, task.attempt, err);
                return ProcessResult::error_with_retry_attempts(task, max_attempts);
            }
        };

        if let Err(err) = account.sync(&cloud.relayer, Some(to_index)).await {
            tracing::warn!("[report task: {}] failed to sync account {}, attempt: {}. Error: {}", id, account_id, task.attempt, err);
            return ProcessResult::error_with_retry_attempts(task, max_attempts);
        }

        let info = account.info(cloud.relayer_fee).await;
        let sk = match account.export_key().await {
            Ok(sk) => sk,
            Err(err) => {
                tracing::warn!("[report task: {}] failed to export key from account {}, attempt: {}. Error: {}", id, account_id, task.attempt, err);
                return ProcessResult::error_with_retry_attempts(task, max_attempts);
            }
        };

        reports.push( AccountReport {
            id: info.id,
            description: info.description,
            balance: info.balance,
            max_transfer_amount: info.max_transfer_amount,
            address: info.address,
            sk,
        });

        if i % 10 == 0 {
            tracing::info!("[report task: {}] {} % processed", id, (i * 100) / count)
        }
    }

    let report = Report {
        timestamp: timestamp(),
        pool_index: to_index,
        accounts: reports,
    };

    tracing::info!("[report task: {}] processed successfully", id);
    ProcessResult::success(task, report)
}

struct ProcessResult {
    delete: bool,
    update: Option<ReportTask>
}

impl ProcessResult {
    fn success(task: ReportTask, report: Report) -> ProcessResult {
        let task = ReportTask {
            status: ReportStatus::Completed,
            report: Some(report),
            ..task
        };
        ProcessResult {
            delete: true,
            update: Some(task),
        }
    }

    fn delete_from_queue() -> ProcessResult {
        ProcessResult {
            delete: true,
            update: None,
        }
    }

    fn error_with_retry_attempts(task: ReportTask, max_attempts: u32) -> ProcessResult {
        if task.attempt >= max_attempts {
            return ProcessResult::error_without_retry(task);
        }

        let task = ReportTask {
            attempt: task.attempt + 1,
            ..task
        };
        ProcessResult {
            delete: false,
            update: Some(task),
        }
    }

    fn error_without_retry(task: ReportTask) -> ProcessResult {
        let task = ReportTask {
            status: ReportStatus::Failed,
            ..task
        };
        ProcessResult {
            delete: true,
            update: Some(task),
        }
    }
}