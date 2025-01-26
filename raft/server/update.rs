use raft::raft_client::RaftClient;
use raft::AppendEntriesRequest;
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;

use crate::{
    rpc_util::calculate_rpc_server_dst,
    state::{self, reset_leader_state},
};

pub mod raft {
    tonic::include_proto!("raft");
}

/** Time between hearbeats. Keep this one order of magnitude lower than ELECTION_TIMEOUT_MS.
 * to ensure no election starts if the leader is alive. */
const HEARTBEAT_TIMEOUT_MS: u128 = 20;

#[derive(Clone)]
pub enum WaitFor {
    Majority,
    Retries(u32),
}

pub async fn maybe_send_update_all(state: Arc<AsyncRwLock<state::State>>) {
    let s = state.read().await;
    if s.role != state::Role::Leader {
        return;
    }
    if s.last_heartbeat_timestamp_ms + HEARTBEAT_TIMEOUT_MS > crate::util::get_current_time_ms() {
        // No need to send an update since we've sent one recently
        return;
    }
    drop(s);
    // Retry once per node, in case of failure we will retry soon-ish
    send_update_all(state, WaitFor::Retries(1)).await;
}

pub async fn send_update_all(state: Arc<AsyncRwLock<state::State>>, wait_for: WaitFor) -> u32 {
    let s: tokio::sync::RwLockReadGuard<'_, state::State> = state.read().await;
    if s.role != state::Role::Leader {
        panic!("Programmer error: send_update_all called on a non-leader node");
    }
    let node_ids = s.node_ids.clone();
    let node_id = s.node_id.clone();
    let majority = (s.node_ids.len() as f32 / 2.0).ceil() as u32;
    drop(s);

    let node_updates_successfully = Arc::new(tokio::sync::Mutex::new(1u32)); // Count self as successful

    let futures = node_ids
        .iter()
        // do not connect to self
        .filter(|id| **id != node_id)
        .map(|id| {
            let id = id.to_string();
            let state = Arc::clone(&state);
            let wait_for = wait_for.clone();
            let node_updates_successfully = Arc::clone(&node_updates_successfully);
            tokio::spawn(async move {
                let mut attempt = 0;
                loop {
                    match send_update_node(&id, state.clone()).await {
                        Ok(result) => {
                            let mut count = node_updates_successfully.lock().await;
                            *count += 1;
                            println!("{} nodes have accepted the update", count);
                            return Ok::<bool, ()>(result);
                        }
                        Err(e) => {
                            println!("Error sending update to node {}: {}", id, e);
                            attempt += 1;
                            match wait_for {
                                WaitFor::Majority => {
                                    let current_updates = *node_updates_successfully.lock().await;
                                    if current_updates >= majority {
                                        // It's ok to bail out early if we've reached majority with other nodes
                                        return Ok(false);
                                    }
                                    // sleep for a bit before retrying
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100))
                                        .await;
                                    continue;
                                }
                                WaitFor::Retries(n) => {
                                    if attempt >= n {
                                        return Ok(false);
                                    }
                                }
                            }
                        }
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    // Wait for all the futures to complete
    for future in futures {
        let _ = future.await.unwrap();
    }

    let successful_responses = *node_updates_successfully.lock().await;

    let s = state.read().await;
    if successful_responses >= majority {
        println!("Majority of nodes have accepted the update");
        // Index of the entry that the leader believe is the latest
        // and the majority of followers agree
        let new_commit_index = s.get_log().len() as u32;
        println!("Updating commit index to {}", new_commit_index);
        drop(s);

        {
            let mut s = state.write().await;
            s.volatile.commit_index = new_commit_index;
            s.apply_committed();
        }
    } else {
        // It's fine to bail out if WaitFor::Retries, we will retry forever soon-ish
        // for trying to reach failed nodes
        println!("Less than majority of nodes have accepted the update");
    }

    let mut s = state.write().await;
    // Mark the time the last heartbeat was sent
    s.last_heartbeat_timestamp_ms = crate::util::get_current_time_ms();
    return successful_responses;
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
            // Current leader is deposed
            reset_leader_state(state);
            state.set_current_term(response.term);
            state.volatile_leader = None;
        } else {
            // The follower has rejected because it's missing the previous log entry
            // or the log entry is not at the expected term
            // we need to decrement the next index in order to find a previous log entry
            // that the follower has
            let next_index = *state
                .volatile_leader
                .as_mut()
                .unwrap()
                .next_index
                .get(dst_id)
                .unwrap();
            state.volatile_leader.as_mut().unwrap().next_index.insert(
                dst_id.to_string(),
                next_index.saturating_sub(1), // Use saturating_sub to prevent underflow
            );
        }
    }
    if response.success {
        match &mut state.volatile_leader {
            Some(leader_state) => {
                leader_state.match_index.insert(
                    dst_id.to_string(),
                    request.prev_log_index + request.entries.len() as u32,
                );
                leader_state.next_index.insert(
                    dst_id.to_string(),
                    request.prev_log_index + request.entries.len() as u32 + 1,
                );
            }
            None => {
                // This can happen in a race when the leader is deposed
                // as consequence of a different node update
                eprintln!("update_leader_state called on a non-leader node");
            }
        }
    }
}

#[cfg(test)]
mod test_update_leader_state {
    use state::init_leader_state;

    use super::*;

    #[tokio::test]
    async fn failure_deposed() {
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
    async fn success() {
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
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().next_index.get("2"),
            // Before the update the leader believes node 2 has no log entries
            Some(&1)
        );

        update_leader_state(&"2", &mut state, &request, &response).await;

        assert_eq!(state.role, state::Role::Leader);
        assert_eq!(state.get_current_term(), 1);
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().match_index.get("2"),
            // After the update the leader believes node 2 has one log entry
            Some(&1)
        );
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().next_index.get("2"),
            // After the update the leader believes node 2 has one log entry
            Some(&2)
        );
    }

    #[tokio::test]
    async fn failure_prev_entry_not_accepted() {
        let mut state = state::init_state(3, "0", None);
        init_leader_state(&mut state);
        state.role = state::Role::Leader;
        state.set_current_term(1);
        state
            .volatile_leader
            .as_mut()
            .unwrap()
            .next_index
            .insert("2".to_string(), 6);
        state
            .volatile_leader
            .as_mut()
            .unwrap()
            .match_index
            .insert("2".to_string(), 0);

        let response = raft::AppendEntriesResponse {
            term: 1,
            success: false,
        };

        let request = tonic::Request::new(AppendEntriesRequest {
            term: 0,
            leader_id: "0".to_string(),
            prev_log_index: 5,
            prev_log_term: 1,
            entries: vec![raft::LogEntry {
                term: 1,
                command: Some(raft::log_entry::Command::Noop(raft::Noop {})),
            }],
            leader_commit: 0,
        });
        let request = request.get_ref();

        assert_eq!(
            state.volatile_leader.as_ref().unwrap().match_index.get("2"),
            Some(&0)
        );
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().next_index.get("2"),
            // Before the update the leader believes node 2 has 5 log entries
            Some(&6)
        );

        update_leader_state(&"2", &mut state, &request, &response).await;

        assert_eq!(state.role, state::Role::Leader);
        assert_eq!(state.get_current_term(), 1);
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().match_index.get("2"),
            // No change
            Some(&0)
        );
        assert_eq!(
            state.volatile_leader.as_ref().unwrap().next_index.get("2"),
            // Decrement the next index by one
            Some(&5)
        );
    }
}
