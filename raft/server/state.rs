use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

enum Command {
    WriteVar { name: String, value: i32 },
    DeleteVar { name: String },
}

struct LogEntry {
    term: u32,
    command: Command,
}

#[derive(Default)]
struct PersistedState {
    current_term: u32,
    voted_for: Option<u32>,
    log: Vec<LogEntry>,
}

#[derive(Default)]
struct VolatileState {
    commit_index: u32,
    last_applied: u32,
}

#[derive(Default)]
struct VolatileLeaderState {
    next_index: Vec<u32>,
    match_index: Vec<u32>,
}

#[derive(Default)]
pub struct StateMachine {
    pub vars: std::collections::HashMap<String, i32>,
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}

impl Default for Role {
    fn default() -> Self {
        Role::Follower
    }
}

#[derive(Default)]
pub struct State {
    persisted: PersistedState,
    volatile: VolatileState,
    volatile_leader: Option<VolatileLeaderState>,
    pub state_machine: StateMachine,

    // State about the current machine
    /** Role of the node */
    pub role: Role,
    /** The last time that a heartbeat was received from a leader, in this machine local time.
     * 0 if the current machine is the leader. */
    pub last_received_heartbeat_timestamp_us: u128,
    /** The id of the current node */
    pub id: u32,
}

pub fn init_state() -> Arc<RwLock<State>> {
    return Arc::new(RwLock::new(State::default()));
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_state_empty() {
        let state = init_state();
        let state_guard = state.read().unwrap();
        assert_eq!(state_guard.persisted.current_term, 0);
        assert_eq!(state_guard.volatile.commit_index, 0);
        assert_eq!(state_guard.volatile.last_applied, 0);
        assert_eq!(state_guard.state_machine.vars.len(), 0);
        assert_eq!(state_guard.role, Role::Follower);
    }
}
