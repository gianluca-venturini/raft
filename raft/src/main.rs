mod server;
mod state;

fn main() {
    let state = state::init_state();
    server::main().unwrap();
}
