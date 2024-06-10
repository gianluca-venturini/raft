enum Command {
    WriteVar { name: String, value: i32 },
    DeleteVar { name: String },
}

struct LogEntry {
    term: u32,
    command: Command,
}

struct PersistedState {
    current_term: u32,
    voted_for: Option<u32>,
    log: Vec<LogEntry>,
}

struct VolatileState {
    commit_index: u32,
    last_applied: u32,
}

struct VolatileLeaderState {
    next_index: Vec<u32>,
    match_index: Vec<u32>,
}

struct StateMachine {
    vars: std::collections::HashMap<String, i32>,
}

pub struct State {
    persisted: PersistedState,
    volatile: VolatileState,
    volatile_leader: Option<VolatileLeaderState>,
    state_machine: StateMachine,
}

pub fn init_state() -> State {
    State {
        persisted: PersistedState {
            current_term: 0,
            voted_for: None,
            log: Vec::new(),
        },
        volatile: VolatileState {
            commit_index: 0,
            last_applied: 0,
        },
        volatile_leader: None,
        state_machine: StateMachine {
            vars: std::collections::HashMap::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_state_empty() {
        assert_eq!(init_state().persisted.current_term, 0);
        assert_eq!(init_state().volatile.commit_index, 0);
        assert_eq!(init_state().volatile.last_applied, 0);
        assert_eq!(init_state().state_machine.vars.len(), 0);
    }
}
