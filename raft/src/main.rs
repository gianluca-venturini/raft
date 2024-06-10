use std::sync::{Arc, RwLock};

mod state;

mod server;

#[tokio::main]
async fn main() {
    let state = state::init_state();
    server::start_server(state).await.unwrap();
}
