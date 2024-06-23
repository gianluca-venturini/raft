use std::time::{SystemTime, UNIX_EPOCH};

/** Get the current system time in milliseconds */
pub fn get_current_time_ms() -> u128 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_millis()
}
