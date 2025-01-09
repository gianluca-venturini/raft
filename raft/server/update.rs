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
    if s.role != state::Role::Leader {
        return;
    }
    let node_ids = s.node_ids.clone();
    drop(s);  // release lock before spawning threads

    let futures = node_ids
        .iter()
        // do not connect to self
        .filter(|id| id != &node_id)
        .map(|id| {
            let id = id.to_string();
            let node_id = node_id.to_string();
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                update_node(&id, &node_id, state).await
            })
        })
        .collect::<Vec<_>>();

    let mut successful_responses = 1; // Count self as successful
    for future in futures {
        if let Ok(Ok(success)) = future.await {
            if success {
                successful_responses += 1;
            }
        }
    }

    // If majority of nodes responded successfully, update commit index
    {
        let mut s = state.lock().await;
        let majority = s.node_ids.len() / 2;
        if successful_responses > majority {
            if let Some(last_log_index) = s.get_log().len().checked_sub(1) {
                s.volatile.commit_index = last_log_index as u32;
            }
        }
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
        state::Command::Noop => {
            raft::log_entry::Command::Noop(raft::Noop {})
        }
    };

    raft::LogEntry {
        term: entry.term,
        command: Some(command),
    }
}

pub async fn update_node(dst_id: &str, id: &str, state: Arc<AsyncMutex<state::State>>) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: only send partial log rather than the whole log based on what every node needs
    let s = state.lock().await;
    let term = s.get_current_term();
    let entries: Vec<raft::LogEntry> = s.get_log().iter().map(convert_log_entry).collect();

    // Get the index and term of the entry preceding new ones
    let prev_log_index = if s.get_log().is_empty() { 0 } else { (s.get_log().len() - 1) as u32 };
    let prev_log_term = if prev_log_index == 0 { 0 } else { s.get_log()[prev_log_index as usize - 1].term };
    let leader_commit = s.volatile.commit_index;
    drop(s);  // release lock before network call

    println!("Sending update to node {} with term {} and entries {:?}", dst_id, term, entries);
    let mut client = RaftClient::connect(calculate_rpc_server_dst(dst_id)).await?;

    let request = tonic::Request::new(AppendEntriesRequest {
        term,
        leader_id: id.to_string(),
        prev_log_index,
        prev_log_term,
        entries,
        leader_commit,
    });

    let response = client.append_entries(request).await?;
    println!("response={:?}", response);

    // TODO: handle rejected updates

    Ok(response.get_ref().success)
}
