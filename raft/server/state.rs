use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use crate::util::get_current_time_ms;

enum Command {
    WriteVar { name: String, value: i32 },
    DeleteVar { name: String },
}

struct LogEntry {
    term: u32,
    command: Command,
}

#[derive(Default)]
pub struct PersistedState {
    pub current_term: u32,
    pub voted_for: Option<String>,
    pub log: Vec<LogEntry>,
}

#[derive(Default)]
pub struct VolatileState {
    pub commit_index: u32,
    pub last_applied: u32,
    /** Believed Id of the leader node */
    pub leader_id: Option<String>,
}

#[derive(Default)]
struct VolatileLeaderState {
    pub next_index: Vec<u32>,
    pub match_index: Vec<u32>,
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
    pub persisted: PersistedState,
    pub volatile: VolatileState,
    pub volatile_leader: Option<VolatileLeaderState>,
    pub state_machine: StateMachine,

    // State about the current machine
    /** Role of the node */
    pub role: Role,
    /** The last time that a heartbeat was received from a leader, in this machine local time.
     * 0 if the current machine is the leader. */
    pub last_received_heartbeat_timestamp_ms: u128,
    /** Ids of the other nodes of the ring */
    pub node_ids: Vec<String>,
}

pub fn init_state(num_nodes: u16) -> State {
    let mut state = State::default();
    for i in 0..num_nodes {
        state.node_ids.push(i.to_string());
    }
    // Initialize to now to avoid immediate election
    state.last_received_heartbeat_timestamp_ms = get_current_time_ms();
    return state;
}

mod tests {
    use super::*;

    #[test]
    fn test_init_state_empty() {
        let state = init_state(0);
        assert_eq!(state.persisted.current_term, 0);
        assert_eq!(state.volatile.commit_index, 0);
        assert_eq!(state.volatile.last_applied, 0);
        assert_eq!(state.state_machine.vars.len(), 0);
        assert_eq!(state.node_ids.len(), 0);
        assert_eq!(state.role, Role::Follower);
    }

    #[test]
    fn test_init_node_ids() {
        let state = init_state(3);
        assert_eq!(state.node_ids.len(), 3);
        assert_eq!(state.node_ids[0], "0");
        assert_eq!(state.node_ids[1], "1");
        assert_eq!(state.node_ids[2], "2");
    }
}
