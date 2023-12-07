use crate::scenario::ceramic::util::goose_error;
use goose::GooseError;

pub mod cas_push;
pub mod ceramic;
pub mod ipfs_block_fetch;

pub async fn get_redis_client() -> Result<redis::Client, GooseError> {
    let redis_host =
        std::env::var("REDIS_CONNECTION_STRING").unwrap_or("redis://redis:6379".to_string());
    redis::Client::open(redis_host).map_err(|e| goose_error(e.into()))
}
