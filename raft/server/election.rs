use once_cell::sync::Lazy;
use raft::raft_client::RaftClient;
use raft::RequestVoteRequest;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::cmp::max;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock as AsyncRwLock;

use crate::state::{init_leader_state, reset_leader_state};
use crate::update::{send_update_all, WaitFor};
use crate::{rpc_util::calculate_rpc_server_dst, state, util::get_current_time_ms};

pub mod raft {
    tonic::include_proto!("raft");
}

static RNG: Lazy<Mutex<StdRng>> = Lazy::new(|| Mutex::new(StdRng::from_entropy()));

const ELECTION_TIMEOUT_MS: u64 = 200;

pub async fn maybe_attempt_election(state: Arc<AsyncRwLock<state::State>>) {
    {
        let s = state.read().await;
        // Only consider starting an election if the node is a follower
        if s.role != state::Role::Follower {
            return;
        }
        let elapsed = get_current_time_ms() - s.last_heartbeat_timestamp_ms;
        // Necessary to wait random time to decrease the probability multiple nodes starting an election at the same time
        let wait_time_jitter_ms = {
            let mut rng = RNG.lock().unwrap();
            rng.gen_range(0..=ELECTION_TIMEOUT_MS)
        };
        println!(
            "Last heartbeat ms timestamp: {}",
            s.last_heartbeat_timestamp_ms
        );
        println!("Elapsed ms since heartbeat: {}", elapsed);
        if elapsed < (ELECTION_TIMEOUT_MS + wait_time_jitter_ms).into() {
            return;
        }
    }

    println!("Attempting election");

    let mut s = state.write().await;
    let node_id = s.node_id.clone();
    let last_log_index = s.get_log().len() as u32;
    let last_log_term = s.get_log().last().map_or(0, |entry| entry.term);
    let node_ids = s.node_ids.clone();
    let mut current_term = s.get_current_term();

    s.set_voted_for(Some(node_id.to_string()));
    current_term = s.set_current_term(current_term + 1);

    s.role = state::Role::Candidate;

    drop(s);

    let max_term = Arc::new(Mutex::new(0)); // Max term seen in responses
    let votes = Arc::new(Mutex::new(1)); // Vote for self

    let threads = node_ids
        .iter()
        // do not connect to self since we already voted for self
        .filter(|id| *id != &node_id)
        .map(|id| {
            let id = id.to_string();
            let votes = Arc::clone(&votes);
            let max_term = Arc::clone(&max_term);
            let candidate_id = node_id.clone();
            tokio::spawn(async move {
                let result = request_vote(
                    &id,
                    current_term,
                    last_log_index,
                    last_log_term,
                    &candidate_id,
                )
                .await;
                if let Err(e) = result {
                    println!("Error requesting vote from node {}: {}", id, e);
                    // Not a big deal, will attempt to be elected from majority of nodes
                } else if let Ok((vote_granted, term)) = result {
                    if vote_granted {
                        println!("Received vote from node {}", id);
                        let mut votes = votes.lock().unwrap();
                        *votes += 1;
                    } else {
                        let mut max_term = max_term.lock().unwrap();
                        *max_term = max(*max_term, term);
                        println!("Rejected vote from node {}", id);
                    }
                };
            })
        });

    for thread in threads {
        thread.await.unwrap();
    }

    {
        let mut s = state.write().await;
        let majority = (s.node_ids.len() as f32 / 2.0).ceil() as u32;
        if *votes.lock().unwrap() >= majority {
            println!("Elected leader with majority votes");
            init_leader_state(&mut s);
            let current_term = s.get_current_term();
            // Push Noop as the first term entry to immediately have an entry for this term to
            // communicate to other nodes
            s.append_log_entry(state::LogEntry {
                term: current_term,
                command: state::Command::Noop,
            });
            drop(s);

            println!("Sending update to all nodes to consolidate leadership");
            // Retry few times to maximize the chance to reach majority of nodes
            // do not retry indefinitely and instead revert to follower if not enough nodes respond
            // to allow some other node to take over
            // e.g. if this node is in a small network partition there's no point in retrying indefinitely
            let successful_responses = send_update_all(state.clone(), WaitFor::Retries(3)).await;

            if successful_responses < majority {
                println!("Failed to update the majority of nodes");
                println!("Reverting to follower");
                let mut s = state.write().await;
                reset_leader_state(&mut s);
                return;
            }

            println!("Update sent to majority of nodes - leadership consolidated");
        } else {
            println!("Election lost");
            let max_term = max_term.lock().unwrap();
            if *max_term > s.get_current_term() {
                println!("Updating term to {}", *max_term);
                let current_term = s.get_current_term();
                s.set_current_term(max(*max_term, current_term));
                s.set_voted_for(None);
            }
            println!("Reverting to follower");
            s.role = state::Role::Follower;
        }
    }
}

async fn request_vote(
    dst_id: &str,
    term: u32,
    last_log_index: u32,
    last_log_term: u32,
    id: &str,
) -> Result<(bool, u32), Box<dyn std::error::Error>> {
    let mut client = RaftClient::connect(calculate_rpc_server_dst(dst_id)).await?;
    let request = tonic::Request::new(RequestVoteRequest {
        term,
        candidate_id: id.to_string(),
        last_log_index,
        last_log_term,
    });

    let response = client.request_vote(request).await?;

    println!("response={:?}", response);

    return Ok((response.get_ref().vote_granted, response.get_ref().term));
}
