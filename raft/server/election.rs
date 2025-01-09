use raft::raft_client::RaftClient;
use raft::{AppendEntriesRequest, RequestVoteRequest};
use std::cmp::max;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use once_cell::sync::Lazy;
use super::update::update_node;

use crate::{state, util::get_current_time_ms, rpc_util::calculate_rpc_server_dst};

pub mod raft {
    tonic::include_proto!("raft");
}

static RNG: Lazy<Mutex<StdRng>> = Lazy::new(|| Mutex::new(StdRng::from_entropy()));

pub const ELECTION_TIMEOUT_MS: u64 = 150;

pub async fn maybe_attempt_election(state: Arc<AsyncMutex<state::State>>, node_id: &str) {
    {
        let s = state.lock().await;
        // Only consider starting an election if the node is a follower
        if s.role != state::Role::Follower {
            return;
        }
        let elapsed = get_current_time_ms() - s.last_received_heartbeat_timestamp_ms;
        // Necessary to wait random time to decrease the probability multiple nodes starting an election at the same time
        let wait_time_jitter_ms = {
            let mut rng = RNG.lock().unwrap();
            rng.gen_range(0..=150)
        };
        println!(
            "Last heartbeat ms timestamp: {}",
            s.last_received_heartbeat_timestamp_ms
        );
        println!("Elapsed ms since heartbeat: {}", elapsed);
        if elapsed < (ELECTION_TIMEOUT_MS + wait_time_jitter_ms).into() {
            return;
        }
    }

    println!("Attempting election");

    let (node_ids, current_term) = {
        let mut s = state.lock().await;

        s.set_voted_for(Some(node_id.to_string()));
        let current_term = s.get_current_term();
        s.set_current_term(current_term + 1);
        s.role = state::Role::Candidate;

        (s.node_ids.clone(), s.get_current_term())
    };
    let max_term = Arc::new(Mutex::new(0)); // Max term seen in responses
    let votes = Arc::new(Mutex::new(1)); // Vote for self

    let threads = node_ids
        .iter()
        // do not connect to self since we already voted for self
        .filter(|id| id != &node_id)
        .map(|id| {
            let node_id = node_id.to_string(); // Clone here
            let id = id.to_string();
            let votes = Arc::clone(&votes);
            let max_term = Arc::clone(&max_term);
            tokio::spawn(async move {
                let result = request_vote(&id, current_term, &node_id).await;
                if let Err(e) = result {
                    println!("Error: {}", e);
                    // Not a big deal, will attempt to be elected from majority of nodes
                } else if let Ok((vote_granted, term)) = result {
                    if vote_granted {
                        println!("Received vote");
                        let mut votes = votes.lock().unwrap();
                        *votes += 1;
                    } else {
                        let mut max_term = max_term.lock().unwrap();
                        *max_term = max(*max_term, term);
                        println!("Rejected vote");
                    }
                };
            })
        });

    for thread in threads {
        thread.await.unwrap();
    }

    {
        let mut s = state.lock().await;
        if *votes.lock().unwrap() > s.node_ids.len() / 2 {
            println!("Elected leader with majority votes");
            s.role = state::Role::Leader;
            let current_term = s.get_current_term();
            // Push Noop as the first term entry to immediately have an entry for this term to
            // communicate to other nodes
            s.append_log_entry(state::LogEntry {
                term: current_term,
                command: state::Command::Noop,
            });
            let node_ids = s.node_ids.clone();
            drop(s);

            let threads = node_ids
                .iter()
                // do not connect to self
                .filter(|id| id != &node_id)
                .map(|id| {
                    let id = id.to_string();
                    let node_id = node_id.to_string();
                    let state = Arc::clone(&state);
                    println!("Time to send updates");
                    tokio::spawn(async move {
                        // Send updates to all nodes to inform them of the new leader
                        let _ = update_node(&id, &node_id, state).await;
                    })
                });

            for thread in threads {
                thread.await.unwrap();
            }
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
    id: &str,
) -> Result<(bool, u32), Box<dyn std::error::Error>> {
    let mut client = RaftClient::connect(calculate_rpc_server_dst(dst_id)).await?;
    let request = tonic::Request::new(RequestVoteRequest {
        term,
        candidate_id: id.to_string(),
        // TODO: set these two fields correctly
        last_log_index: 1,
        last_log_term: 1,
    });

    let response = client.request_vote(request).await?;

    println!("response={:?}", response);

    return Ok((response.get_ref().vote_granted, response.get_ref().term));
}
