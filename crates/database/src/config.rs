use std::time::Duration;

pub struct Config {
    pub thread_pool_size: usize,
    pub max_transaction_duration: Duration,
}
