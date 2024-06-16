use std::sync::{Arc, RwLock};
use tokio::task;
use tokio::time::{interval, Duration};

mod server;
mod state;
mod util;
mod web_server;

/** Periodically transition the server role. */
fn spawn_timer(state: Arc<RwLock<state::State>>) {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(1));

        loop {
            interval.tick().await;
            let state_guard = state.read().unwrap();
            println!(
                "Callback called! {}",
                state_guard.last_received_heartbeat_timestamp_us
            );
        }
    });
}

#[tokio::main]
async fn main() {
    let state = state::init_state();
    spawn_timer(state.clone());
    let rpc_server = task::spawn(server::start_rpc_server(state.clone()));
    let web_server = task::spawn(web_server::start_web_server(state.clone()));

    let _ = tokio::try_join!(rpc_server, web_server);
}
