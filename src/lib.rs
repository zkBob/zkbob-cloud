use libzkbob_rs::libzeropool::{native::params::PoolBN256, fawkes_crypto::{backend::bellman_groth16::engines::Bn256, engines::bn256}};

pub mod config;
pub mod errors;
pub mod cloud;
pub mod account;
pub mod helpers;
pub mod relayer;
pub mod web3;
pub mod routes;
pub mod version;
pub mod types;

pub type PoolParams = PoolBN256;
pub type Engine = Bn256;
pub type Fr = bn256::Fr;
pub type Database = kvdb_rocksdb::Database;