use raft::raft_server::{Raft, RaftServer};
use raft::{AppendEntriesRequest, AppendEntriesResponse};
use std::env;
use std::sync::Arc;
use tokio::sync::{watch, RwLock as AsyncRwLock};
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

#[derive(Default)]
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

        let mut reply = raft::RequestVoteResponse {
            term: request.get_ref().term,
            vote_granted: true,
        };
        if request.get_ref().term < s.get_current_term() {
            println!("Vote not granted: candidate term is not up to date");
            reply.term = s.get_current_term();
            reply.vote_granted = false;
        } else if s.get_voted_for().is_some()
            && s.get_voted_for() != Some(request.get_ref().candidate_id.to_string())
            && request.get_ref().term == s.get_current_term()
        {
            println!("Vote not granted: already voted for another candidate in this term");
            reply.vote_granted = false;
        } else {
            println!("Vote granted");
            s.set_voted_for(Some(request.get_ref().candidate_id.to_string()));
            s.set_current_term(request.get_ref().term);
        }

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

    let server = Server::builder()
        .add_service(RaftServer::new(raft))
        .serve_with_shutdown(addr, async {
            shutdown_rx.changed().await.ok();
        });

    println!("RPC server started");

    let send_future = async move { server.await };
    send_future.await?;

    Ok(())
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
    state.last_heartbeat_timestamp_ms = get_current_time_ms();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maybe_append_entries_success_on_empty() {
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
    fn test_maybe_append_entries_success_on_entry_same_term() {
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
    fn test_maybe_append_entries_success_on_entry_different_term() {
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
    fn test_maybe_append_entries_success_commit() {
        let mut state = state::State::default();
        state.append_log_entry(state::LogEntry {
            term: 1,
            command: state::Command::WriteVar {
                name: "x".to_string(),
                value: 1,
            },
        });

        // No new entry is been appended
        let entries = vec![];

        assert_eq!(state.volatile.commit_index, 0);

        let (success, term) = maybe_append_entries(
            &mut state, 1, "0", &entries, 1, 1,
            // The entry already in the log is now committed
            1,
        );

        assert!(success);
        assert_eq!(term, 1);
        assert_eq!(state.volatile.commit_index, 1);
    }

    #[test]
    fn test_maybe_append_entries_success_append_commit() {
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

        let (success, term) = maybe_append_entries(
            &mut state, 3, "0", &entries, 1, 1,
            // The entry that is being appended is alrady committed
            // e.g. the majority of the other followers already successfully appended it
            2,
        );

        assert!(success);
        assert_eq!(term, 3);
        assert_eq!(state.volatile.commit_index, 2);
    }

    #[test]
    fn test_maybe_append_entries_failure_empty() {
        let mut state = state::State::default();

        let entries = vec![raft::LogEntry {
            term: 3,
            command: Some(raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: "y".to_string(),
                value: 1,
            })),
        }];

        let (success, term) = maybe_append_entries(&mut state, 3, "0", &entries, 1, 1, 0);

        // Should not succeed because the log doesn't contain the prev_log_index
        assert!(!success);
        assert_eq!(term, 0);
        assert_eq!(state.get_log().len(), 0);
        assert_eq!(state.volatile.commit_index, 0);
    }

    #[test]
    fn test_maybe_append_entries_failure_prev_index_not_in_log() {
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

        // Should not succeed because the log doesn't contain the prev_log_index
        assert!(!success);
        assert_eq!(term, 1);
        assert_eq!(state.get_log().len(), 1);
        assert_eq!(state.volatile.commit_index, 0);
    }

    #[test]
    fn test_maybe_append_entries_failure_prev_term_different() {
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

        // Should not succeed because the log contains the log index, but the term is different
        assert!(!success);
        assert_eq!(term, 1);
        assert_eq!(state.get_log().len(), 1);
        assert_eq!(state.volatile.commit_index, 0);
    }

    #[test]
    fn test_maybe_append_entries_failure_old_term() {
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

        // Should not succeed because the leader log is older than the follower
        assert!(!success);
        // Should send the updated term in order to inform the leader it should be deposed
        // and not update its term
        assert_eq!(term, 3);
        // Entries are not appended
        assert_eq!(state.get_log().len(), 0);
        assert_eq!(state.volatile.commit_index, 0);
    }
}
