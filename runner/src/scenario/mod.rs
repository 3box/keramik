use crate::scenario::ceramic::util::goose_error;
use goose::GooseError;

pub mod ceramic;
pub mod ipfs_block_fetch;
pub mod recon_sync;

static FIRST_USER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub(crate) async fn get_redis_client() -> Result<redis::Client, GooseError> {
    let redis_host =
        std::env::var("REDIS_CONNECTION_STRING").unwrap_or("redis://redis:6379".to_string());
    redis::Client::open(redis_host).map_err(|e| goose_error(e.into()))
}

/// True if this is the 'leader' i.e. the worker process that should make requests for sharing with others
pub(crate) fn is_goose_lead_worker() -> bool {
    goose::get_worker_id() == 1
}

/// True if this is the lead user on that worker i.e. if you want to do something once for a worker.
/// Only returns true once until being reset. You should cache the result and call `reset_goose_lead_user`
/// if you want to reacquire the lead user status.
pub(crate) fn is_goose_lead_user() -> bool {
    FIRST_USER.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// True if this is the lead worker process and the lead user on that worker i.e. if you want to do something once for the simulation
pub(crate) fn is_goose_global_leader(lead_user: bool) -> bool {
    is_goose_lead_worker() && lead_user
}

/// Reset the lead user flag so another process can act as the lead user in the future
pub(crate) fn reset_goose_lead_user() {
    FIRST_USER.store(true, std::sync::atomic::Ordering::SeqCst);
}
