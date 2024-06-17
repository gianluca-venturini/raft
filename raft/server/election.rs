use once_cell::sync::Lazy;
use raft::raft_client::RaftClient;
use raft::{RequestVoteRequest, RequestVoteResponse};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::cmp::max;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

use crate::{state, util::get_current_time_microseconds};

pub mod raft {
    tonic::include_proto!("raft");
}

static RNG: Lazy<Mutex<StdRng>> = Lazy::new(|| Mutex::new(StdRng::from_entropy()));

const ELECTION_TIMEOUT_US: u128 = 1_000_000;
pub async fn maybe_attempt_election(state: Arc<AsyncMutex<state::State>>, node_id: &str) {
    {
        let s = state.lock().await;
        let elapsed = get_current_time_microseconds() - s.last_received_heartbeat_timestamp_us;
        if elapsed < ELECTION_TIMEOUT_US {
            return;
        }
    }

    // Necessary to wait random time to prevent multiple nodes from starting an election at the same time
    let wait_time_us = {
        let mut rng = RNG.lock().unwrap();
        rng.gen_range(1..=1_000_000)
    };
    println!("Waiting for {} us", wait_time_us);
    tokio::time::sleep(tokio::time::Duration::from_micros(wait_time_us)).await;

    println!("Attempting election");

    let mut s = state.lock().await;

    let votes = Arc::new(Mutex::new(1)); // Vote for self
    s.persisted.voted_for = Some(node_id.to_string());
    let max_term = Arc::new(Mutex::new(0)); // Max term seen in responses

    let current_term = s.persisted.current_term;
    let node_ids = s.node_ids.clone();

    let threads = node_ids
        .iter()
        // do not connect to self since we already voted for self
        .filter(|id| id != &node_id)
        .map(|id| {
            let node_id = node_id.to_string();
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

    if *votes.lock().unwrap() > s.node_ids.len() / 2 {
        println!("Elected leader with majority votes");
        s.role = state::Role::Leader;
    } else {
        println!("Election lost");
        let max_term = max_term.lock().unwrap();
        if *max_term > s.persisted.current_term {
            println!("Updating term to {}", *max_term);
            s.persisted.current_term = max(s.persisted.current_term, *max_term);
        }
        println!("Reverting to follower");
        s.role = state::Role::Follower;
        s.persisted.voted_for = None;
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

/** Calculate the destination RPC url based on the id */
fn calculate_rpc_server_dst(id: &str) -> String {
    // For now keep it simple and assume it's a 0-based integer
    let id_int = id.parse::<u32>().unwrap();
    format!("http://[::1]:{}", 50000 + id_int)
}
