use raft::raft_client::RaftClient;
use raft::AppendEntriesRequest;
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;

use crate::{rpc_util::calculate_rpc_server_dst, state};

pub mod raft {
    tonic::include_proto!("raft");
}

pub async fn maybe_send_update(state: Arc<AsyncRwLock<state::State>>) {
    let s = state.read().await;
    if s.role != state::Role::Leader {
        return;
    }
    let node_ids = s.node_ids.clone();
    let node_id = s.node_id.clone();

    let futures = node_ids
        .iter()
        // do not connect to self
        .filter(|id| **id != node_id)
        .map(|id| {
            let id = id.to_string();
            let state = Arc::clone(&state);
            tokio::spawn(async move { update_node(&id, state).await })
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

    let majority = s.node_ids.len() / 2;
    if successful_responses > majority {
        println!("Majority of nodes have accepted the update");
        // Index of the entry that the leader believe is the latest
        // and the majority of followers agree
        let new_commit_index = s.get_log().len() as u32;
        drop(s);

        {
            let mut s = state.write().await;
            s.volatile.commit_index = new_commit_index;
            s.apply_committed();
        }
    } else {
        // TODO: Handle the case in which not majority of nodes have accepted the update
        println!("Not enough nodes have accepted the update");
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
            raft::log_entry::Command::DeleteVar(raft::DeleteVar { name: name.clone() })
        }
        state::Command::Noop => raft::log_entry::Command::Noop(raft::Noop {}),
    };

    raft::LogEntry {
        term: entry.term,
        command: Some(command),
    }
}

pub async fn update_node(
    dst_id: &str,
    state: Arc<AsyncRwLock<state::State>>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    // TODO: only send partial log rather than the whole log based on what every node needs
    let s = state.read().await;
    let node_id = s.node_id.clone();
    let term = s.get_current_term();
    let entries: Vec<raft::LogEntry> = s.get_log().iter().map(convert_log_entry).collect();

    // Get the index and term of the entry preceding new ones
    // let prev_log_index = if s.get_log().is_empty() { 0 } else { (s.get_log().len() - 1) as u32 };
    // TODO: with partial log this can be larger
    let prev_log_index = 0;
    let prev_log_term = if prev_log_index == 0 {
        0
    } else {
        s.get_log()[prev_log_index as usize - 1].term
    };
    let leader_commit = s.volatile.commit_index;
    drop(s); // release lock before network call

    println!(
        "Sending update to node {} with term {} and entries {:?}",
        dst_id, term, entries
    );
    let mut client = RaftClient::connect(calculate_rpc_server_dst(dst_id)).await?;

    let request = tonic::Request::new(AppendEntriesRequest {
        term,
        leader_id: node_id,
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
