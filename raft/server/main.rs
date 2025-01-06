use election::maybe_attempt_election;
use update::maybe_send_update;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::sync::{watch, Mutex as AsyncMutex};
use tokio::{signal, task};
use tokio::signal::unix::{signal, SignalKind};
use tracing::info;
use tracing_subscriber;

mod election;
mod update;
mod rpc_server;
mod state;
mod util;
mod rpc_util;
mod web_server;

const MAYBE_ATTEMPT_ELECTION_INTERVAL_MS: u64 = 500;

/** Periodically transition the server role. */
fn spawn_timer(state: Arc<AsyncMutex<state::State>>, id: &str) {
    let id = id.to_string();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                election::ELECTION_TIMEOUT_MS / 10,
            ))
            .await;
            maybe_attempt_election(state.clone(), &id).await;
            maybe_send_update(state.clone(), &id).await;
        }
    });
}

#[tokio::main]
async fn main() {
    // tracing_subscriber::fmt()
    //     .with_max_level(tracing::Level::DEBUG)
    //     .init();

    let id = env::var("ID").expect(
        "ID (raft node unique identifier) environment variable is not set or cannot be read",
    );
    let num_nodes: u16 = env::var("NUM_NODES").expect(
        "NUM_NODES (total number of raft nodes) environment variable is not set or cannot be read",
    ).parse()
    .expect("NUM_NODES must be an integer");
    let state = Arc::new(AsyncMutex::new(state::init_state(num_nodes)));

    spawn_timer(state.clone(), &id);

    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let rpc_server = task::spawn(rpc_server::start_rpc_server(
        state.clone(),
        shutdown_rx.clone(),
    ));
    let web_server = task::spawn(web_server::start_web_server(state.clone()));

    let mut sigterm = signal(SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = signal::ctrl_c() => {
            println!("Received Ctrl+C, sending shutdown signal...");
            let _ = shutdown_tx.send(());
        }
        _ = sigterm.recv() => {
            println!("Received SIGTERM, sending shutdown signal...");
            let _ = shutdown_tx.send(());
        }
        _ = rpc_server => {
            println!("RPC server terminated");
        }
        _ = web_server => {
            println!("Web server terminated");
        }
    }
}
