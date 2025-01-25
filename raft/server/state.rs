use std::collections::HashMap; // Add this import
use std::fs::{self};
use std::io::{self, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

use crate::util::get_current_time_ms;
use bincode;
use serde::{Deserialize, Serialize};

const CURRENT_TERM_FILE: &str = "current_term.bin";
const VOTED_FOR_FILE: &str = "voted_for.bin";
const LOG_FILE: &str = "log.bin";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    WriteVar { name: String, value: i32 },
    DeleteVar { name: String },
    Noop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u32,
    pub command: Command,
}

#[derive(Default, Clone, Serialize)]
struct PersistedState {
    current_term: u32,
    voted_for: Option<String>,
    log: Vec<LogEntry>,
}

#[derive(Default)]
pub struct VolatileState {
    pub commit_index: u32,
    pub last_applied: u32,
    /** Believed Id of the leader node */
    pub leader_id: Option<String>,
}

#[derive(Default)]
pub struct VolatileLeaderState {
    pub next_index: HashMap<String, u32>,
    pub match_index: HashMap<String, u32>,
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
    pub volatile: VolatileState,
    pub volatile_leader: Option<VolatileLeaderState>,
    pub state_machine: StateMachine,
    storage_path: Option<PathBuf>,

    // State about the current machine
    /** Role of the node */
    pub role: Role,
    /** For followers: the last time that a heartbeat was received from the leader in this machine local time.
     * For leader: the last time that a heartbeat was sent to the followers in this machine local time. */
    pub last_heartbeat_timestamp_ms: u128,
    /** Ids of the other nodes of the ring */
    pub node_ids: Vec<String>,
    /** Id of the current node */
    pub node_id: String,
}

impl State {
    pub fn apply_committed(&mut self) {
        while self.volatile.last_applied < self.volatile.commit_index {
            let next_index = (self.volatile.last_applied + 1) as usize;
            if let Some(entry) = self.persisted.log.get((next_index as u32 - 1) as usize) {
                println!("Applying committed log entry: {}", next_index);
                match &entry.command {
                    Command::WriteVar { name, value } => {
                        println!("Applying committed write: {} = {}", name, value);
                        self.state_machine.vars.insert(name.clone(), *value);
                    }
                    Command::DeleteVar { name } => {
                        println!("Applying committed delete: {}", name);
                        self.state_machine.vars.remove(&name.clone());
                    }
                    Command::Noop => {
                        println!("Applying noop");
                    }
                }
                self.volatile.last_applied = next_index as u32;
            }
        }
        println!(
            "Applied committed entries up to index {}",
            self.volatile.last_applied
        );
    }

    pub fn get_current_term(&self) -> u32 {
        self.persisted.current_term
    }

    pub fn get_voted_for(&self) -> Option<String> {
        self.persisted.voted_for.clone()
    }

    pub fn get_log(&self) -> &Vec<LogEntry> {
        &self.persisted.log
    }

    pub fn set_current_term(&mut self, term: u32) -> u32 {
        self.persisted.current_term = term;
        if let Err(e) = self.atomic_write_to_file(CURRENT_TERM_FILE, &term.to_le_bytes()) {
            eprintln!("Failed to persist current_term: {}", e);
        }
        return self.persisted.current_term;
    }

    pub fn set_voted_for(&mut self, node_id: Option<String>) -> Option<String> {
        self.persisted.voted_for = node_id.clone();
        let bytes = if let Some(id) = &node_id {
            id.as_bytes().to_vec()
        } else {
            vec![]
        };
        if let Err(e) = self.atomic_write_to_file(VOTED_FOR_FILE, &bytes) {
            eprintln!("Failed to persist voted_for: {}", e);
        }
        return self.persisted.voted_for.clone();
    }

    pub fn append_log_entry(&mut self, entry: LogEntry) {
        self.persisted.log.push(entry.clone());
        // TODO: append to log file rather than rewriting the whole log every time
        if let Ok(serialized) = bincode::serialize(&self.persisted.log) {
            if let Err(e) = self.atomic_write_to_file(LOG_FILE, &serialized) {
                eprintln!("Failed to persist log: {}", e);
            }
        }
    }

    pub fn truncate_log(&mut self, index: usize) {
        self.persisted.log = self.persisted.log[..index].to_vec();
        if let Ok(serialized) = bincode::serialize(&self.persisted.log) {
            if let Err(e) = self.atomic_write_to_file(LOG_FILE, &serialized) {
                eprintln!("Failed to persist log: {}", e);
            }
        }
    }

    fn get_file_path(&self, filename: &str) -> Option<PathBuf> {
        self.storage_path.as_ref().map(|path| path.join(filename))
    }

    fn atomic_write_to_file(&self, filename: &str, content: &[u8]) -> io::Result<()> {
        if let Some(file_path) = self.get_file_path(filename) {
            let dir = file_path
                .parent()
                .ok_or(io::Error::new(io::ErrorKind::Other, "Invalid path"))?;

            let mut temp_file = NamedTempFile::new_in(dir)?;
            temp_file.write_all(content)?;
            temp_file.flush()?;
            temp_file.persist(file_path)?;
        }
        Ok(())
    }

    fn read_from_file(&self, filename: &str) -> io::Result<Vec<u8>> {
        match self.get_file_path(filename) {
            Some(file_path) => fs::read(file_path),
            None => Ok(vec![]),
        }
    }

    fn load_persisted_state(&mut self) -> io::Result<()> {
        // Load current_term
        if let Ok(bytes) = self.read_from_file(CURRENT_TERM_FILE) {
            if bytes.len() == 4 {
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes);
                self.persisted.current_term = u32::from_le_bytes(arr);
                println!("Loaded current_term: {}", self.persisted.current_term);
            }
        }

        // Load voted_for
        if let Ok(bytes) = self.read_from_file(VOTED_FOR_FILE) {
            if !bytes.is_empty() {
                self.persisted.voted_for = Some(String::from_utf8_lossy(&bytes).to_string());
                println!(
                    "Loaded voted_for: {}",
                    self.persisted.voted_for.as_ref().unwrap()
                );
            }
        }

        // Load log
        if let Ok(bytes) = self.read_from_file(LOG_FILE) {
            if bytes.is_empty() {
                println!("No existing log file found, starting with empty log");
            } else {
                match bincode::deserialize(&bytes) {
                    Ok(log) => {
                        self.persisted.log = log;
                        println!("Loaded log: {:?}", self.persisted.log);
                    }
                    Err(e) => {
                        // This is a serious error that indicates data corruption
                        // TODO: handle this simply leaving the log empty
                        // and reset completely the state deleting all the other files
                        panic!(
                            "Failed to deserialize log file (possible corruption): {}",
                            e
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn init_state(num_nodes: u16, node_id: &str, storage_path: Option<String>) -> State {
    let mut state = State::default();
    state.storage_path = storage_path.map(PathBuf::from);

    // Create storage directory if needed
    if let Some(path) = &state.storage_path {
        println!("Initializing storage directory: {:?}", path);
        if let Err(e) = fs::create_dir_all(path) {
            eprintln!("Failed to create storage directory: {}", e);
        }
    }

    // Load persisted state if available
    if let Err(e) = state.load_persisted_state() {
        eprintln!("Failed to load persisted state: {}", e);
    }

    for i in 0..num_nodes {
        state.node_ids.push(i.to_string());
    }
    state.last_heartbeat_timestamp_ms = get_current_time_ms();
    state.node_id = node_id.to_string();
    state
}

pub fn init_leader_state(state: &mut State) {
    state.role = Role::Leader;
    state.volatile_leader = Some(VolatileLeaderState::default());
    state.volatile_leader.as_mut().unwrap().next_index = state
        .node_ids
        .iter()
        .map(|id| (id.clone(), state.get_log().len() as u32 + 1))
        .collect();
    state.volatile_leader.as_mut().unwrap().match_index =
        state.node_ids.iter().map(|id| (id.clone(), 0)).collect();
}

#[cfg(test)]
mod test_init_state {
    use crate::state::*;

    #[test]
    fn test_init_state_empty() {
        let state = init_state(0, "0", None);
        assert_eq!(state.get_current_term(), 0);
        assert_eq!(state.volatile.commit_index, 0);
        assert_eq!(state.volatile.last_applied, 0);
        assert_eq!(state.state_machine.vars.len(), 0);
        assert_eq!(state.node_ids.len(), 0);
        assert_eq!(state.role, Role::Follower);
        assert_eq!(state.node_id, "0");
    }

    #[test]
    fn test_init_node_ids() {
        let state = init_state(3, "0", None);
        assert_eq!(state.node_ids.len(), 3);
        assert_eq!(state.node_ids[0], "0");
        assert_eq!(state.node_ids[1], "1");
        assert_eq!(state.node_ids[2], "2");
        assert_eq!(state.node_id, "0");
    }

    #[test]
    fn test_apply_committed_write() {
        let mut state = init_state(1, "0", None);
        state.append_log_entry(LogEntry {
            term: 1,
            command: Command::WriteVar {
                name: "a".to_string(),
                value: 1,
            },
        });
        state.volatile.commit_index = 1;

        assert_eq!(state.volatile.last_applied, 0);

        state.apply_committed();

        assert_eq!(state.volatile.last_applied, 1);
        assert_eq!(state.state_machine.vars.get("a"), Some(&1));
    }

    #[test]
    fn test_override_committed_write() {
        let mut state = init_state(1, "0", None);
        state.append_log_entry(LogEntry {
            term: 1,
            command: Command::WriteVar {
                name: "a".to_string(),
                value: 1,
            },
        });
        state.append_log_entry(LogEntry {
            term: 1,
            command: Command::WriteVar {
                name: "a".to_string(),
                value: 2,
            },
        });

        state.volatile.commit_index = 1;
        state.apply_committed();
        assert_eq!(state.volatile.last_applied, 1);
        assert_eq!(state.state_machine.vars.get("a"), Some(&1));

        state.volatile.commit_index = 2;
        state.apply_committed();
        assert_eq!(state.volatile.last_applied, 2);
        assert_eq!(state.state_machine.vars.get("a"), Some(&2));
    }
}
