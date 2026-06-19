use std::time::Duration;

use database::{Config, Database};
use storage::implementations::heed::datastore::HeedStorageEngine;

#[tokio::test]
async fn test_transaction() {
    let storage = HeedStorageEngine::open("./local").unwrap();

    let db = Database::new(
        storage,
        Config {
            max_transaction_duration: Duration::from_secs(1),
            thread_pool_size: 4,
        },
    )
    .await
    .unwrap();

    let tx = db
        .transaction(database::TransactionMode::ReadOnly)
        .await
        .unwrap();

    tx.commit();
}
