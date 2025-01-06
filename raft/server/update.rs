use raft::raft_client::RaftClient;
use raft::{AppendEntriesRequest, RequestVoteRequest};
use std::cmp::max;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use once_cell::sync::Lazy;

use crate::{state, util::get_current_time_ms, rpc_util::calculate_rpc_server_dst};

pub mod raft {
    tonic::include_proto!("raft");
}

pub async fn maybe_send_update(state: Arc<AsyncMutex<state::State>>, node_id: &str) {
    let s = state.lock().await;
    let current_term = s.persisted.current_term;
    if s.role != state::Role::Leader {
        return;
    }
    let node_ids = s.node_ids.clone();
    let threads = node_ids
        .iter()
        // do not connect to self
        .filter(|id| id != &node_id)
        .map(|id| {
            let id = id.to_string();
            let node_id = node_id.to_string();
            tokio::spawn({
                let log = s.persisted.log.clone();
                async move {
                    let _ = update_node(&id, &node_id, current_term, log).await;
                }
            })
        });

    for thread in threads {
        thread.await.unwrap();
    }
}

fn convert_log_entry(entry: &state::LogEntry) -> raft::LogEntry {
    let command = match &entry.command {
        state::Command::WriteVar { name, value } => {
            raft::log_entry::Command::WriteVar(raft::WriteVar {
                name: name.clone(),
                value: *value,
            })
        }
        state::Command::DeleteVar { name } => {
            raft::log_entry::Command::DeleteVar(raft::DeleteVar {
                name: name.clone(),
            })
        }
    };

    raft::LogEntry {
        term: entry.term,
        command: Some(command),
    }
}

async fn update_node(dst_id: &str, id: &str, term: u32, log: Vec<state::LogEntry>) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = RaftClient::connect(calculate_rpc_server_dst(dst_id)).await?;
    
    // TODO: only send partial log rather than the whole log based on what every node needs
    let entries = log.iter().map(convert_log_entry).collect();
    let request = tonic::Request::new(AppendEntriesRequest {
        term,
        leader_id: id.to_string(),
        entries,
    });

    let response = client.append_entries(request).await?;
    println!("response={:?}", response);

    Ok(())
}
