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

        let mut reply = raft::AppendEntriesResponse {
            term: 0,
            success: true,
        };

        {
            let mut state = self.state.write().await;
            state.last_received_heartbeat_timestamp_ms = get_current_time_ms();
            state.volatile.leader_id = Some(request.get_ref().leader_id.to_string());

            let prev_log_index = request.get_ref().prev_log_index as usize;
            if prev_log_index > 0 && state.get_log().len() <= prev_log_index - 1 {
                println!("Append entries failed: log is too small for comparison");
                reply.success = false;
            } else if prev_log_index > 0 && state.get_log()[(prev_log_index - 1) as usize].term != request.get_ref().prev_log_term {
                println!("Append entries failed: log term does not match");
                reply.success = false;
            } else {
                println!("Append entries succeeded");
                // Note: We need to add a new method to State to handle this log truncation
                state.truncate_log(prev_log_index);
                
                let entries: Vec<state::LogEntry> = request
                    .get_ref()
                    .entries
                    .iter()
                    .map(convert_proto_entry)
                    .collect();
                
                for entry in entries {
                    state.append_log_entry(entry);
                }
                
                state.set_current_term(request.get_ref().term);
                state.volatile.commit_index = request.get_ref().leader_commit;
                state.apply_committed();
            }

            reply.term = state.get_current_term();
        }

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
        }
        if s.get_voted_for().is_some()
            && s.get_voted_for() != Some(request.get_ref().candidate_id.to_string())
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
