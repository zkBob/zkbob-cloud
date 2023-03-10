use actix_cors::Cors;
use actix_web::{web::{Data, JsonConfig, get, post}, App, middleware::Logger, HttpServer, HttpResponse};
use libzkbob_rs::libzeropool::{fawkes_crypto::backend::bellman_groth16::Parameters, POOL_PARAMS};
use zkbob_cloud::{Engine, config::Config, errors::CloudError, version, cloud::{cloud::ZkBobCloud, signup, short_info, generate_shielded_address}};
use zkbob_utils_rs::{telemetry::telemetry, contracts::pool::Pool, tracing};


pub fn get_params(path: &str) -> Parameters<Engine> {
    let data = std::fs::read(path).expect("failed to read file with snark params");
    Parameters::<Engine>::read(&mut data.as_slice(), true, true)
        .expect("failed to parse file with snark params")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = Config::get().expect("failed to parse config");
    telemetry::setup(&config.telemetry);

    let params = Data::new(get_params(&config.transfer_params_path));
    let pool_params = Data::new(POOL_PARAMS.clone());
    let pool = Data::new(Pool::new(&config.web3).unwrap());
    let pool_id = pool.pool_id().await.expect("failed to get pool_id from contract");
    tracing::info!("pool_id: {}", pool_id);

    let cloud = Data::new(ZkBobCloud::new(config.clone(), pool_id).expect("failed to init cloud"));

    let host = config.host.clone();
    let port = config.port;
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
            .app_data(params.clone())
            .app_data(pool_params.clone())
            .app_data(json_config)
            .app_data(pool.clone())
            .app_data(cloud.clone())
            .route("/", get().to(|| HttpResponse::Ok()))
            .route("/version", get().to(version::version))
            .route("/signup", post().to(signup))
            .route("/account", get().to(short_info))
            .route("/generateAddress", get().to(generate_shielded_address))
    })
    .bind((host, port))?
    .run()
    .await
}
