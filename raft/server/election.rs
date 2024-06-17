use once_cell::sync::Lazy;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::{Arc, Mutex, RwLock};

use crate::{state, util::get_current_time_microseconds};

static RNG: Lazy<Mutex<StdRng>> = Lazy::new(|| Mutex::new(StdRng::from_entropy()));

const ELECTION_TIMEOUT_US: u128 = 1_000_000;
pub async fn maybe_attempt_election(state: Arc<RwLock<state::State>>) {
    {
        let s = state.read().unwrap();
        let elapsed = get_current_time_microseconds() - s.last_received_heartbeat_timestamp_us;
        if elapsed < ELECTION_TIMEOUT_US {
            return;
        }
    }

    // Necessary to wait random time to prevent multiple nodes from starting an election at the same time
    let wait_time_us = {
        let mut rng = RNG.lock().unwrap();
        rng.gen_range(1..=100_000)
    };
    println!("Waiting for {} us", wait_time_us);
    tokio::time::sleep(tokio::time::Duration::from_micros(wait_time_us)).await;

    println!("Attempting election");

    // TODO: implement election logic
}
