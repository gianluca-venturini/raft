use election::maybe_attempt_election;
use std::env;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{watch, RwLock as AsyncRwLock};
use tokio::{signal, task};
use update::maybe_send_update_all;

mod election;
mod rpc_server;
mod rpc_util;
mod state;
mod update;
mod util;
mod web_server;

/** Sleep time on the thread that runs elections and updates.
 * Keep this value significantly lower than the other timeouts. */
const SLEEP_MS: u64 = 5;

/** Periodically transition the server role. */
fn spawn_timer(state: Arc<AsyncRwLock<state::State>>, no_election: bool) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(SLEEP_MS)).await;
            if !no_election {
                maybe_attempt_election(state.clone()).await;
            }
            maybe_send_update_all(state.clone()).await;
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
    let storage_path = env::var_os("STORAGE_PATH").map(|p| p.into_string().unwrap());
    let no_election = env::var("NO_ELECTION").is_ok();
    let state = Arc::new(AsyncRwLock::new(state::init_state(
        num_nodes,
        &id,
        storage_path,
    )));

    spawn_timer(state.clone(), no_election);

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
        result = rpc_server => {
            match result {
                Ok(server_result) => println!("RPC server thread terminated with result: {:?}", server_result),
                Err(e) => println!("RPC server thread terminated with error: {:?}", e),
            }
        }
        result = web_server => {
            match result {
                Ok(server_result) => println!("Web server thread terminated with result: {:?}", server_result),
                Err(e) => println!("Web server thread terminated with error: {:?}", e),
            }
        }
    }
}
