use super::*;
use std::future::Future;

const MAX_DEADLOCK_RETRIES: u32 = 4;

pub fn is_deadlock(e: &Error) -> bool {
    let Error::Internal(inner) = e else {
        return false;
    };
    inner
        .downcast_ref::<sqlx::Error>()
        .and_then(|e| e.as_database_error())
        .and_then(|d| d.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
        .is_some_and(|d| d.number() == 1213)
}

pub async fn retry_deadlock<T, F, Fut>(mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut tries = 0;
    loop {
        match op().await {
            Err(e) if is_deadlock(&e) && tries < MAX_DEADLOCK_RETRIES => {
                tries += 1;
                log::warn!("deadlock, retrying ({tries})");
                tokio::time::sleep(std::time::Duration::from_millis(20 * u64::from(tries))).await;
            }
            other => return other,
        }
    }
}
