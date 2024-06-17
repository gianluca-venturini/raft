use election::maybe_attempt_election;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task;
use tokio::time::{interval, Duration};

mod election;
mod rpc_server;
mod state;
mod util;
mod web_server;

/** Periodically transition the server role. */
fn spawn_timer(state: Arc<AsyncMutex<state::State>>, id: &str) {
    let id = id.to_string();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(1));

        loop {
            interval.tick().await;
            {
                let state_guard = state.lock().await;
                println!(
                    "Callback called! {}",
                    state_guard.last_received_heartbeat_timestamp_us
                );
            }
            maybe_attempt_election(state.clone(), &id).await;
        }
    });
}

#[tokio::main]
async fn main() {
    let id = env::var("ID").expect(
        "ID (raft node unique identifier) environment variable is not set or cannot be read",
    );
    let state = Arc::new(AsyncMutex::new(state::init_state()));
    {
        // TODO: make the node ids configurable
        let mut s = state.lock().await;
        s.node_ids = vec!["0".to_string(), "1".to_string(), "2".to_string()];
    }

    spawn_timer(state.clone(), &id);
    let rpc_server = task::spawn(rpc_server::start_rpc_server(state.clone()));
    let web_server = task::spawn(web_server::start_web_server(state.clone()));

    let _ = tokio::try_join!(rpc_server, web_server);
}
