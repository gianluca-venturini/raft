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
            tokio::spawn(async move { send_update_node(&id, state).await })
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

pub async fn send_update_node(
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
    let request = request.get_ref();

    let response = client.append_entries(request.clone()).await?;
    let response = response.get_ref();
    println!("response={:?}", response);

    {
        let mut s = state.write().await;
        update_leader_state(&dst_id, &mut s, &request, &response).await;
    }

    Ok(response.success)
}

/** Update the current leader node state  */
pub async fn update_leader_state(
    dst_id: &str,
    state: &mut state::State,
    request: &raft::AppendEntriesRequest,
    response: &raft::AppendEntriesResponse,
) {
    if !response.success {
        if response.term > state.get_current_term() {
            state.role = state::Role::Follower;
            state.set_current_term(response.term);
            state.volatile_leader = None;
        }
        // TODO: implement all the other cases
    }
    if response.success {
        if let Some(leader_state) = &mut state.volatile_leader {
            leader_state.match_index.insert(
                dst_id.to_string(),
                request.prev_log_index + request.entries.len() as u32,
            );
        }
        // TODO: implement all the other cases
    }
}

#[cfg(test)]
mod tests {
    use state::init_leader_state;

    use super::*;

    #[tokio::test]
    async fn test_update_leader_state_deposed() {
        let mut state = state::init_state(3, "0", None);
        init_leader_state(&mut state);
        state.role = state::Role::Leader;
        state.set_current_term(1);

        let response = raft::AppendEntriesResponse {
            term: 2,
            success: false,
        };

        let request = tonic::Request::new(AppendEntriesRequest {
            term: 0,
            leader_id: "0".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        });
        let request = request.get_ref();
        update_leader_state(&"2", &mut state, &request, &response).await;

        // If the response term is higher, the node should step down from leader
        assert_eq!(state.role, state::Role::Follower);
        assert_eq!(state.get_current_term(), 2);
    }

    #[tokio::test]
    async fn test_update_leader_state_success() {
        let mut state = state::init_state(3, "0", None);
        init_leader_state(&mut state);
        state.role = state::Role::Leader;
        state.set_current_term(1);

        let response = raft::AppendEntriesResponse {
            term: 1,
            success: true,
        };

        let request = tonic::Request::new(AppendEntriesRequest {
            term: 0,
            leader_id: "0".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![raft::LogEntry {
                term: 1,
                command: Some(raft::log_entry::Command::Noop(raft::Noop {})),
            }],
            leader_commit: 0,
        });
        let request = request.get_ref();

        assert_eq!(
            state.volatile_leader.as_ref().unwrap().match_index.get("2"),
            // Before the update the leader believes node 2 has no log entries
            Some(&0)
        );

        update_leader_state(&"2", &mut state, &request, &response).await;

        assert_eq!(state.role, state::Role::Leader);
        assert_eq!(state.get_current_term(), 1);
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().match_index.get("2"),
            // After the update the leader believes node 2 has one log entry
            Some(&1)
        );
    }
}
