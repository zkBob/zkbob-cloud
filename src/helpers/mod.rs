use std::time::{SystemTime, UNIX_EPOCH};

use libzkbob_rs::libzeropool::fawkes_crypto::ff_uint::Num;

use crate::Fr;

pub mod db;

pub trait AsU64Amount {
    fn as_u64_amount(&self) -> u64;
}

// It is applicable to tx amount only because tx amount is exactly 64 bit
impl AsU64Amount for Num<Fr> {
    fn as_u64_amount(&self) -> u64 {
        self.to_uint().0.0[0]
    }
}

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Default::default())
        .as_secs()
}