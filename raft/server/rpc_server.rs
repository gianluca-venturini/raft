use raft::raft_server::{Raft, RaftServer};
use raft::{AppendEntriesRequest, AppendEntriesResponse};
use std::env;
use std::sync::Arc;
use tokio::sync::{watch, Mutex as AsyncMutex};
use tonic::{transport::Server, Request, Response, Status};

use crate::state;
use crate::util::get_current_time_ms;

pub mod raft {
    tonic::include_proto!("raft");
}

#[derive(Default)]
pub struct MyRaft {
    state: Arc<AsyncMutex<state::State>>,
}

#[tonic::async_trait]
impl Raft for MyRaft {
    async fn append_entries(
        &self,
        request: Request<AppendEntriesRequest>,
    ) -> Result<Response<AppendEntriesResponse>, Status> {
        println!("append_entries request={:?}", request);

        {
            let mut state = self.state.lock().await;
            // Set the current timestamp as the last received heartbeat timestamp
            state.last_received_heartbeat_timestamp_ms = get_current_time_ms();
            state.volatile.leader_id = Some(request.get_ref().leader_id.to_string());
        }

        // TODO: implement this response
        let reply = raft::AppendEntriesResponse {
            term: 1,
            success: true,
        };

        Ok(Response::new(reply))
    }

    async fn request_vote(
        &self,
        request: Request<raft::RequestVoteRequest>,
    ) -> Result<Response<raft::RequestVoteResponse>, Status> {
        println!("request_vote request={:?}", request);

        let mut s = self.state.lock().await;

        let mut reply = raft::RequestVoteResponse {
            term: request.get_ref().term,
            vote_granted: true,
        };
        if request.get_ref().term < s.persisted.current_term {
            println!("Vote not granted: candidate term is not up to date");
            reply.term = s.persisted.current_term;
            reply.vote_granted = false;
        }
        if s.persisted.voted_for.is_some()
            && s.persisted.voted_for != Some(request.get_ref().candidate_id.to_string())
        {
            println!("Vote not granted: already voted for another candidate in this term");
            reply.vote_granted = false;
        } else {
            println!("Vote granted");
            s.persisted.voted_for = Some(request.get_ref().candidate_id.to_string());
            s.persisted.current_term = request.get_ref().term;
        }

        Ok(Response::new(reply))
    }
}

pub async fn start_rpc_server(
    state: Arc<AsyncMutex<state::State>>,
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
