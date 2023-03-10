use actix_web::{web::Data, HttpResponse};
use serde::Serialize;

use crate::{config::Config, errors::CloudError};



#[derive(Serialize)]
pub struct VersionResponse {
    #[serde(rename = "ref")]
    pub ref_name: Option<String>,
    #[serde(rename = "commitHash")]
    pub commit_hash: Option<String>,
}

pub async fn version(
    config: Data<Config>,
) -> Result<HttpResponse, CloudError> {
    let response = VersionResponse {
        ref_name: config.version.ref_name.clone(),
        commit_hash: config.version.commit_hash.clone(),
    };
    Ok(HttpResponse::Ok()
        .content_type("application/json;")
        .json(response))
}