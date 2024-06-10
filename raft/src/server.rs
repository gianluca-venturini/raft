use raft::raft_server::{Raft, RaftServer};
use raft::{AppendEntriesRequest, AppendEntriesResponse};
use std::sync::{Arc, RwLock};
use tonic::{transport::Server, Request, Response, Status};

use crate::state;
use crate::util::get_current_time_microseconds;

pub mod raft {
    tonic::include_proto!("raft");
}

#[derive(Default)]
pub struct MyRaft {
    state: Arc<RwLock<state::State>>,
}

#[tonic::async_trait]
impl Raft for MyRaft {
    async fn append_entries(
        &self,
        request: Request<AppendEntriesRequest>,
    ) -> Result<Response<AppendEntriesResponse>, Status> {
        println!("request={:?}", request);

        {
            // Set the current timestamp as the last received heartbeat timestamp
            self.state
                .write()
                .unwrap()
                .last_received_heartbeat_timestamp_us = get_current_time_microseconds();
        }

        let reply = raft::AppendEntriesResponse {
            term: 1,
            success: true,
        };

        Ok(Response::new(reply))
    }
}

pub async fn start_server(
    state: Arc<RwLock<state::State>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse().unwrap();
    let raft = MyRaft { state: state };

    Server::builder()
        .add_service(RaftServer::new(raft))
        .serve(addr)
        .await?;

    Ok(())
}
