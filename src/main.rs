use actix_cors::Cors;
use actix_web::{web::{JsonConfig, get, post, Data}, App, middleware::Logger, HttpServer, HttpResponse};
use libzkbob_rs::{
    libzeropool::fawkes_crypto::{backend::bellman_groth16::Parameters, ff_uint::Num},
    pools::Pool as PoolId
};
use web3::types::U256;
use zkbob_cloud::{Engine, config::Config, errors::CloudError, version, cloud::ZkBobCloud, routes::{signup, account_info, list_accounts, generate_shielded_address, history, transfer, transaction_status, calculate_fee, export_key, transaction_trace, generate_report, report, clean_reports, import, delete_account}, helpers::AsU32PoolId};
use zkbob_utils_rs::{telemetry::telemetry, contracts::pool::Pool, tracing, configuration::Web3Settings};

pub fn get_params(path: &str) -> Parameters<Engine> {
    let data = std::fs::read(path).expect("failed to read file with snark params");
    Parameters::<Engine>::read(&mut data.as_slice(), true, true)
        .expect("failed to parse file with snark params")
}

async fn init_pool(settings: &Web3Settings) -> (Pool, PoolId) {
    let pool = Pool::new(settings).expect("failed to init pool");
    let chain_id = pool.chain_id().await.expect("failed to fetch chain id");
    let pool_id = pool.pool_id().await.expect("failed to get pool_id from contract");

    let (pool, pool_id) = {
        // sepolia pool exception
        if pool_id == Num::from(0) && chain_id == U256::from(11155111) {
            (pool, PoolId::SepoliaBOB)
        } else {
            let pool_id = PoolId::from_pool_id(pool_id.as_u32_pool_id()).expect("unknown pool");
            (pool, pool_id)
        }
    };
    tracing::info!("Pool: {}", pool_id.human_readable());
    (pool, pool_id)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = Data::new(Config::get().expect("failed to parse config"));
    telemetry::setup(&config.telemetry);

    let params = get_params(&config.transfer_params_path);
    let (pool, pool_id) = init_pool(&config.web3).await;

    let host = config.host.clone();
    let port = config.port;

    let cloud = ZkBobCloud::new(config.clone(), pool, pool_id, params).await.expect("failed to init cloud");

    tracing::info!(
        "starting webserver at http://{}:{}",
        &host,
        &port
    );

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST"])
            .allow_any_header()
            .max_age(3600);

        let json_config = JsonConfig::default()
            .error_handler(|err, _| CloudError::BadRequest(err.to_string()).into());

        App::new()
            .wrap(cors)
            .wrap(Logger::new("%r %s %b %T %r support-id=%{zkbob-support-id}i"))
            .app_data(json_config)
            .app_data(cloud.clone())
            .app_data(config.clone())
            .route("/", get().to(HttpResponse::Ok))
            .route("/version", get().to(version::version))
            .route("/signup", post().to(signup))
            .route("/import", post().to(import))
            .route("deleteAccount", post().to(delete_account))
            .route("/accounts", get().to(list_accounts))
            .route("/transactionTrace", get().to(transaction_trace))
            .route("/export", get().to(export_key))
            .route("/generateReport", post().to(generate_report))
            .route("/report", get().to(report))
            .route("/cleanReports", post().to(clean_reports))
            .route("/account", get().to(account_info))
            .route("/generateAddress", get().to(generate_shielded_address))
            .route("/history", get().to(history))
            .route("/transfer", post().to(transfer))
            .route("/transactionStatus", get().to(transaction_status))
            .route("/calculateFee", get().to(calculate_fee))
    })
    .bind((host, port))?
    .run()
    .await
}
