use crate::scenario::ceramic::util::goose_error;
use goose::GooseError;

pub mod ceramic;
pub mod ipfs_block_fetch;

pub(crate) async fn get_redis_client() -> Result<redis::Client, GooseError> {
    let redis_host =
        std::env::var("REDIS_CONNECTION_STRING").unwrap_or("redis://redis:6379".to_string());
    redis::Client::open(redis_host).map_err(|e| goose_error(e.into()))
}

/// True if this is the 'leader' i.e. the worker than should make requests for sharing with others
pub(crate) fn is_goose_leader() -> bool {
    goose::get_worker_id() == 1
}

static FIRST_USER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// If this is the first user on the worker, return true. Used to limit requests that should only be done
/// by a single user on a worker.
pub(crate) fn is_first_goose_user() -> bool {
    FIRST_USER.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// Reset the first user flag. Used to allow a single user to become the leader again.
pub(crate) fn reset_first_goose_user() {
    FIRST_USER.store(true, std::sync::atomic::Ordering::SeqCst);
}
