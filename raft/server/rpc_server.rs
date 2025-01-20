use raft::raft_server::{Raft, RaftServer};
use raft::{AppendEntriesRequest, AppendEntriesResponse};
use std::env;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::{watch, RwLock as AsyncRwLock};
use tonic::transport::Error as TransportError;
use tonic::{transport::Server, Request, Response, Status};

use crate::state;
use crate::util::get_current_time_ms;

pub mod raft {
    tonic::include_proto!("raft");
}

fn convert_proto_entry(entry: &raft::LogEntry) -> state::LogEntry {
    let command = match &entry.command {
        Some(raft::log_entry::Command::WriteVar(w)) => state::Command::WriteVar {
            name: w.name.clone(),
            value: w.value,
        },
        Some(raft::log_entry::Command::DeleteVar(d)) => state::Command::DeleteVar {
            name: d.name.clone(),
        },
        Some(raft::log_entry::Command::Noop(_)) => state::Command::Noop,
        None => panic!("Log entry command cannot be empty"),
    };
    state::LogEntry {
        term: entry.term,
        command,
    }
}

#[derive(Default, Clone)]
pub struct MyRaft {
    state: Arc<AsyncRwLock<state::State>>,
}

#[tonic::async_trait]
impl Raft for MyRaft {
    async fn append_entries(
        &self,
        request: Request<AppendEntriesRequest>,
    ) -> Result<Response<AppendEntriesResponse>, Status> {
        println!("append_entries request={:?}", request);

        let mut s = self.state.write().await;

        if s.role == state::Role::Leader && s.get_current_term() < request.get_ref().term {
            // The current leader has been deposed by a leader node with a higher term
            s.role = state::Role::Follower;
        }

        s.last_heartbeat_timestamp_ms = get_current_time_ms();

        let (success, term) = maybe_append_entries(
            &mut s,
            request.get_ref().term,
            &request.get_ref().leader_id,
            &request.get_ref().entries,
            request.get_ref().prev_log_index,
            request.get_ref().prev_log_term,
            request.get_ref().leader_commit,
        );

        let reply = raft::AppendEntriesResponse { term, success };

        Ok(Response::new(reply))
    }

    async fn request_vote(
        &self,
        request: Request<raft::RequestVoteRequest>,
    ) -> Result<Response<raft::RequestVoteResponse>, Status> {
        println!("request_vote request={:?}", request);

        let mut s = self.state.write().await;

        // Resetting the heartbeat is useful to minimize the candidates at the same time
        s.last_heartbeat_timestamp_ms = get_current_time_ms();

        let (term, vote_granted) = calculate_vote(
            &mut s,
            request.get_ref().term,
            &request.get_ref().candidate_id,
        );

        let reply = raft::RequestVoteResponse { term, vote_granted };
        Ok(Response::new(reply))
    }
}

pub async fn start_rpc_server(
    state: Arc<AsyncRwLock<state::State>>,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port =
        env::var("RPC_PORT").expect("RPC_PORT environment variable is not set or cannot be read");
    let addr = format!("[::1]:{}", port).parse().unwrap();
    let raft = MyRaft { state: state };

    loop {
        let server = Server::builder()
            .add_service(RaftServer::new(raft.clone()))
            .serve_with_shutdown(addr, async {
                shutdown_rx.changed().await.ok();
            });

        println!("RPC server started");

        match server.await {
            Ok(_) => {
                println!("RPC server terminated");
                return Ok(());
            }
            Err(e) => {
                if e.to_string().contains("transport error") {
                    // This usually happens when the port is already in use
                    // waiting on the port to be released by the OS
                    eprintln!("Transport error on port {}, retrying: {}", port, e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
                eprintln!("Failed to start RPC server on port {}: {}", port, e);
                return Err(e.into());
            }
        }
    }
}

fn maybe_append_entries(
    state: &mut state::State,
    term: u32,
    leader_id: &str,
    entries: &Vec<raft::LogEntry>,
    prev_log_index: u32,
    prev_log_term: u32,
    leader_commit: u32,
) -> (bool, u32) {
    state.volatile.leader_id = Some(leader_id.to_string());

    if term < state.get_current_term() {
        println!("Append entries failed: term is older than current term");
        return (false, state.get_current_term());
    }

    let prev_log_index = prev_log_index as usize;
    if prev_log_index > 0 && state.get_log().len() <= prev_log_index - 1 {
        println!("Append entries failed: log is too small for comparison");
        return (false, state.get_current_term());
    }

    if prev_log_index > 0 && state.get_log()[(prev_log_index - 1) as usize].term != prev_log_term {
        println!("Append entries failed: log term does not match");
        return (false, state.get_current_term());
    }

    println!("Append entries succeeded");
    state.truncate_log(prev_log_index);

    let log_entries: Vec<state::LogEntry> = entries.iter().map(convert_proto_entry).collect();

    for entry in log_entries {
        state.append_log_entry(entry);
    }

    state.set_current_term(term);
    state.volatile.commit_index = leader_commit;
    state.apply_committed();

    (true, state.get_current_term())
}

fn calculate_vote(
    state: &mut state::State,
    candidate_term: u32,
    candidate_id: &str,
) -> (u32, bool) {
    if candidate_term < state.get_current_term() {
        println!("Vote not granted: candidate term is not up to date");
        return (state.get_current_term(), false);
    }

    if state.get_voted_for().is_some()
        && state.get_voted_for() != Some(candidate_id.to_string())
        && candidate_term == state.get_current_term()
    {
        println!("Vote not granted: already voted for another candidate in this term");
        return (candidate_term, false);
    }

    // TODO: check lastLogTerm and lastLogIndex before granting vote

    println!("Vote granted");
    state.set_voted_for(Some(candidate_id.to_string()));
    state.set_current_term(candidate_term);
    (candidate_term, true)
}

#[cfg(test)]
mod test_maybe_append_entries {
    use super::maybe_append_entries;
    use crate::{rpc_server::raft, state};

    #[test]
    fn success_on_empty() {
        let mut state = state::State::default();

        let entries = vec![raft::LogEntry {
            term: 1,
            command: Some(raft::log_entry::Command::Noop(raft::Noop {})),
        }];

        let (success, term) = maybe_append_entries(&mut state, 1, "0", &entries, 0, 0, 0);

        assert!(success);
        assert_eq!(term, 1);
        assert_eq!(state.get_log().len(), 1);
        assert_eq!(state.get_log()[0].command, state::Command::Noop);
    }

    #[test]
    fn success_on_entry_same_term() {
        let mut state = state::State::default();
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        let entries = vec![raft::LogEntry {
            term: 1,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 1, "0", &entries, 1, 1, 0);

        assert!(success);
        assert_eq!(term, 1);
        assert_eq!(state.get_log().len(), 2);
        assert_eq!(
            state.get_log()[0].command,
            state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            }
        );
        assert_eq!(
            state.get_log()[1].command,
            state::Command::WriteVar {
                name: "y".to_string(),
                value: 1,
            }
        );
    }

    #[test]
    fn success_on_entry_different_term() {
        let mut state = state::State::default();
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        let entries = vec![raft::LogEntry {
            term: 3,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 3, "0", &entries, 1, 1, 0);

        assert!(success);
        assert_eq!(term, 3);
        assert_eq!(state.volatile.commit_index, 0);
        assert_eq!(state.get_log().len(), 2);
        assert_eq!(state.get_log()[0].term, 1);
        assert_eq!(state.get_log()[1].term, 3);
        assert_eq!(
            state.get_log()[0].command,
            state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            }
        );
        assert_eq!(
            state.get_log()[1].command,
            state::Command::WriteVar {
                name: "y".to_string(),
                value: 1,
            }
        );
    }

    #[test]
    fn success_commit() {
        let mut state = state::State::default();
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        let entries = vec![];
        assert_eq!(state.volatile.commit_index, 0);

        let (success, term) = maybe_append_entries(&mut state, 1, "0", &entries, 1, 1, 1);

        assert!(success);
        assert_eq!(term, 1);
        assert_eq!(state.volatile.commit_index, 1);
    }

    #[test]
    fn success_append_commit() {
        let mut state = state::State::default();
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        let entries = vec![raft::LogEntry {
            term: 3,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        assert_eq!(state.volatile.commit_index, 0);

        let (success, term) = maybe_append_entries(&mut state, 3, "0", &entries, 1, 1, 2);

        assert!(success);
        assert_eq!(term, 3);
        assert_eq!(state.volatile.commit_index, 2);
    }

    #[test]
    fn failure_empty() {
        let mut state = state::State::default();

        let entries = vec![raft::LogEntry {
            term: 3,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 3, "0", &entries, 1, 1, 0);

        assert!(!success);
        assert_eq!(term, 0);
        assert_eq!(state.get_log().len(), 0);
        assert_eq!(state.volatile.commit_index, 0);
    }

    #[test]
    fn failure_prev_index_not_in_log() {
        let mut state = state::State::default();
        state.set_current_term(1);
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        let entries = vec![raft::LogEntry {
            term: 3,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 3, "0", &entries, 2, 1, 0);

        assert!(!success);
        assert_eq!(term, 1);
        assert_eq!(state.get_log().len(), 1);
        assert_eq!(state.volatile.commit_index, 0);
    }

    #[test]
    fn failure_prev_term_different() {
        let mut state = state::State::default();
        state.set_current_term(1);
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        let entries = vec![raft::LogEntry {
            term: 3,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 3, "0", &entries, 1, 2, 0);

        assert!(!success);
        assert_eq!(term, 1);
        assert_eq!(state.get_log().len(), 1);
        assert_eq!(state.volatile.commit_index, 0);
    }

    #[test]
    fn failure_old_term() {
        let mut state = state::State::default();
        state.set_current_term(3);

        let entries = vec![raft::LogEntry {
            term: 1,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 1, "0", &entries, 0, 0, 0);

        assert!(!success);
        assert_eq!(term, 3);
        assert_eq!(state.get_log().len(), 0);
        assert_eq!(state.volatile.commit_index, 0);
    }
}

#[cfg(test)]
mod test_calculate_vote {
    use super::calculate_vote;
    use crate::state;

    #[test]
    fn first_vote() {
        let mut state = state::State::default();
        state.set_current_term(0);

        let (term, vote_granted) = calculate_vote(&mut state, 1, "candidate1");

        assert!(vote_granted);
        assert_eq!(term, 1);
        assert_eq!(state.get_current_term(), 1);
        assert_eq!(state.get_voted_for(), Some("candidate1".to_string()));
    }

    #[test]
    fn deny_old_term() {
        let mut state = state::State::default();
        state.set_current_term(2);

        let (term, vote_granted) = calculate_vote(&mut state, 1, "candidate1");

        assert!(!vote_granted);
        assert_eq!(term, 2);
        assert_eq!(state.get_current_term(), 2);
        assert_eq!(state.get_voted_for(), None);
    }

    #[test]
    fn deny_already_voted() {
        let mut state = state::State::default();
        state.set_current_term(1);
        state.set_voted_for(Some("candidate1".to_string()));

        let (term, vote_granted) = calculate_vote(&mut state, 1, "candidate2");

        assert!(!vote_granted);
        assert_eq!(term, 1);
        assert_eq!(state.get_current_term(), 1);
        assert_eq!(state.get_voted_for(), Some("candidate1".to_string()));
    }

    #[test]
    fn grant_same_candidate() {
        let mut state = state::State::default();
        state.set_current_term(1);
        state.set_voted_for(Some("candidate1".to_string()));

        let (term, vote_granted) = calculate_vote(&mut state, 1, "candidate1");

        assert!(vote_granted);
        assert_eq!(term, 1);
        assert_eq!(state.get_current_term(), 1);
        assert_eq!(state.get_voted_for(), Some("candidate1".to_string()));
    }

    #[test]
    fn grant_new_term() {
        let mut state = state::State::default();
        state.set_current_term(1);
        state.set_voted_for(Some("candidate1".to_string()));

        let (term, vote_granted) = calculate_vote(&mut state, 2, "candidate2");

        assert!(vote_granted);
        assert_eq!(term, 2);
        assert_eq!(state.get_current_term(), 2);
        assert_eq!(state.get_voted_for(), Some("candidate2".to_string()));
    }
}
