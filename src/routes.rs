use std::str::FromStr;

use actix_web::{web::{Json, Data, Query}, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::{errors::CloudError, types::{SignupRequest, SignupResponse, AccountInfoRequest, GenerateAddressResponse, TransferRequest, TransferResponse, TransactionStatusRequest, CalculateFeeRequest, CalculateFeeResponse, ExportKeyResponse, HistoryRecord, TransactionStatusResponse, ReportRequest, ReportResponse, ImportRequest}, cloud::{ZkBobCloud, types::{Transfer, AccountImportData}}, helpers::invert};

pub async fn signup(
    request: Json<SignupRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;

    let id = invert(request.id.as_ref().map(|id| parse_uuid(id)))?;
    let sk = invert(request.sk.as_ref().map(hex::decode))?;
    
    let account_id = cloud.new_account(request.0.description, id, sk).await?;

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn import(
    request: Json<ImportRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let accounts = request.iter().map(|account| {
        Ok(AccountImportData {
            id: parse_uuid(&account.id)?,
            description: account.description.clone(),
            sk: hex::decode(&account.sk)?
        })
    }).collect::<Result<Vec<_>, CloudError>>()?;
    
    cloud.import_accounts(accounts).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn delete_account(
    request: Json<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let id = parse_uuid(&request.id)?;
    cloud.delete_account(id).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn list_accounts(
    bearer: BearerAuth,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let accounts = cloud.list_accounts().await?;
    Ok(HttpResponse::Ok().json(accounts))
}

pub async fn account_info(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_uuid(&request.id)?;
    let account_info = cloud
        .account_info(account_id)
        .await?;
    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn generate_shielded_address(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_uuid(&request.id)?;
    let address = cloud.generate_address(account_id).await?;
    Ok(HttpResponse::Ok().json(GenerateAddressResponse { address }))
}

pub async fn history(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_uuid(&request.id)?;
    let txs = cloud.history(account_id).await?;
    Ok(HttpResponse::Ok().json(HistoryRecord::prepare_records(txs)))
}

pub async fn transfer(
    request: Json<TransferRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_uuid(&request.account_id)?;

    let transaction_id = cloud.transfer(Transfer{
        id: request.transaction_id.clone().unwrap_or(Uuid::new_v4().as_hyphenated().to_string()),
        account_id,
        amount: request.amount,
        to: request.to.clone(),
    }).await?;

    Ok(HttpResponse::Ok().json(TransferResponse{ transaction_id }))
}

pub async fn transaction_trace(
    request: Query<TransactionStatusRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let parts = cloud.transfer_status(&request.transaction_id).await?;
    Ok(HttpResponse::Ok().json(parts))
}

pub async fn transaction_status(
    request: Query<TransactionStatusRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let parts = cloud.transfer_status(&request.transaction_id).await?;
    Ok(HttpResponse::Ok().json(TransactionStatusResponse::from(parts)))
}

pub async fn calculate_fee(
    request: Query<CalculateFeeRequest>,
    cloud: Data<ZkBobCloud>
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_uuid(&request.account_id)?;
    let (transaction_count, total_fee) = cloud.calculate_fee(account_id, request.amount).await?;
    Ok(HttpResponse::Ok().json(CalculateFeeResponse{transaction_count, total_fee}))
}

pub async fn export_key(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let account_id = parse_uuid(&request.id)?;
    let sk = cloud.export_key(account_id).await?;
    Ok(HttpResponse::Ok().json(ExportKeyResponse { sk }))
}

pub async fn generate_report(
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let id = cloud.generate_report().await?;
    Ok(HttpResponse::Ok().json(ReportResponse {
        id: id.as_hyphenated().to_string(),
        status: None,
        report: None,
    }))
}

pub async fn report(
    request: Query<ReportRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    let report_id = parse_uuid(&request.id)?;
    match cloud.get_report(report_id).await? {
        Some(task) => Ok(HttpResponse::Ok().json(ReportResponse {
            id: report_id.as_hyphenated().to_string(),
            status: Some(task.status),
            report: task.report,
        })),
        None => Err(CloudError::ReportNotFound)
    }
}

pub async fn clean_reports(
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;
    cloud.clean_reports().await?;
    Ok(HttpResponse::Ok().finish())
}

fn parse_uuid(id: &str) -> Result<Uuid, CloudError> {
    Uuid::from_str(id).map_err(|err| {
        tracing::debug!("failed to parse uuid: {}", err);
        CloudError::IncorrectAccountId
    })
}