use config::{File, FileFormat, Environment};
use serde::{Serialize, Deserialize};
use zkbob_utils_rs::configuration::{TelemetrySettings, Version, Web3Settings};

use crate::errors::CloudError;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WorkerConfig {
    pub max_attempts: u32,
    pub queue_delay_sec: u32,
    pub queue_hidden_sec: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub transfer_params_path: String,
    pub db_path: String,
    pub relayer_url: String,
    pub redis_url: String,
    pub admin_token: String,
    pub telemetry: TelemetrySettings,
    pub version: Version,
    pub web3: Web3Settings,
    pub send_worker: WorkerConfig,
    pub status_worker: WorkerConfig,
}

impl Config {
    pub fn get() -> Result<Config, CloudError> {
        let mut config = config::Config::builder()
            .add_source(File::new("./configuration/base.yaml", FileFormat::Yaml));

        config = match std::env::var("CONFIG_FILE") {
            Ok(config_path) => config.add_source(File::new(&config_path, FileFormat::Yaml)),
            Err(_) => config,
        };

        config = config.add_source(Environment::default().separator("__"));
        Ok(config.build()?.try_deserialize()?)
    }
}