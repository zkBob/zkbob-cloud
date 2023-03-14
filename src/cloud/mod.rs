use std::str::FromStr;

use actix_web::{web::{Json, Data, Query}, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use uuid::Uuid;
use zkbob_utils_rs::tracing;

use crate::errors::CloudError;

use self::{types::{SignupRequest, SignupResponse, AccountInfoRequest, GenerateAddressResponse, TransferRequest, Transfer, TransferResponse, TransferStatusRequest, CalculateFeeRequest, CalculateFeeResponse}, cloud::ZkBobCloud};

pub mod cloud;
pub mod types;
mod db;
mod queue;
mod send_worker;
mod status_worker;
mod cleanup;

pub async fn signup(
    request: Json<SignupRequest>,
    cloud: Data<ZkBobCloud>,
    bearer: BearerAuth,
) -> Result<HttpResponse, CloudError> {
    cloud.validate_token(bearer.token())?;

    let id = match request.0.id {
        Some(id) => {
            Some(Uuid::from_str(&id).map_err(|_| {
                CloudError::IncorrectAccountId
            })?)
        },
        None => None
    };

    let sk = match request.0.sk {
        Some(sk) => {
            let sk = hex::decode(sk)
                    .map_err(|err| CloudError::BadRequest(format!("failed to parse sk: {}", err)))?;
                
            sk.try_into()
                .map_err(|_| CloudError::BadRequest(format!("failed to parse sk")))?
        },
        None => None
    };
    
    let account_id = cloud.new_account(request.0.description, id, sk).await?;

    Ok(HttpResponse::Ok().json(SignupResponse {
        account_id: account_id.to_string(),
    }))
}

pub async fn short_info(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_account_id(&request.id)?;
    let account_info = cloud
        .account_info(account_id)
        .await?;
    Ok(HttpResponse::Ok().json(account_info))
}

pub async fn generate_shielded_address(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_account_id(&request.id)?;
    let address = cloud.generate_address(account_id).await?;
    Ok(HttpResponse::Ok().json(GenerateAddressResponse { address }))
}

pub async fn history(
    request: Query<AccountInfoRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_account_id(&request.id)?;
    let history = cloud.history(account_id).await?;
    Ok(HttpResponse::Ok().json(history))
}

pub async fn transfer(
    request: Json<TransferRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_account_id(&request.account_id)?;

    let request_id = cloud.transfer(Transfer{
        id: request.request_id.clone().unwrap_or(Uuid::new_v4().as_hyphenated().to_string()),
        account_id,
        amount: request.amount,
        to: request.to.clone(),
    }).await?;

    Ok(HttpResponse::Ok().json(TransferResponse{ request_id }))
}

pub async fn transfer_status(
    request: Query<TransferStatusRequest>,
    cloud: Data<ZkBobCloud>,
) -> Result<HttpResponse, CloudError> {
    let parts = cloud.transfer_status(&request.request_id).await?;
    Ok(HttpResponse::Ok().json(parts))
}

pub async fn calculate_fee(
    request: Json<CalculateFeeRequest>,
    cloud: Data<ZkBobCloud>
) -> Result<HttpResponse, CloudError> {
    let account_id = parse_account_id(&request.account_id)?;
    let (transaction_count, total_fee) = cloud.calculate_fee(account_id, request.amount).await?;
    Ok(HttpResponse::Ok().json(CalculateFeeResponse{transaction_count, total_fee}))
}

fn parse_account_id(account_id: &str) -> Result<Uuid, CloudError> {
    Uuid::from_str(account_id).map_err(|err| {
        tracing::debug!("failed to parse account id: {}", err);
        CloudError::IncorrectAccountId
    })
}