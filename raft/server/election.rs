use raft::raft_client::RaftClient;
use raft::RequestVoteRequest;
use std::cmp::max;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

use crate::{state, util::get_current_time_ms};

pub mod raft {
    tonic::include_proto!("raft");
}

const ELECTION_TIMEOUT_MS: u128 = 1_000;
pub async fn maybe_attempt_election(state: Arc<AsyncMutex<state::State>>, node_id: &str) {
    {
        let s = state.lock().await;
        // Only consider starting an election if the node is a follower
        if s.role != state::Role::Follower {
            return;
        }
        let elapsed = get_current_time_ms() - s.last_received_heartbeat_timestamp_ms;
        println!(
            "Last heartbeat ms timestamp: {}",
            s.last_received_heartbeat_timestamp_ms
        );
        println!("Elapsed ms since heartbeat: {}", elapsed);
        if elapsed < ELECTION_TIMEOUT_MS {
            return;
        }
    }

    println!("Attempting election");

    let (node_ids, current_term) = {
        let mut s = state.lock().await;

        s.persisted.voted_for = Some(node_id.to_string());
        s.persisted.current_term += 1;

        (s.node_ids.clone(), s.persisted.current_term)
    };
    let max_term = Arc::new(Mutex::new(0)); // Max term seen in responses
    let votes = Arc::new(Mutex::new(1)); // Vote for self

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

    {
        let mut s = state.lock().await;
        if *votes.lock().unwrap() > s.node_ids.len() / 2 {
            println!("Elected leader with majority votes");
            s.role = state::Role::Leader;
        } else {
            println!("Election lost");
            let max_term = max_term.lock().unwrap();
            if *max_term > s.persisted.current_term {
                println!("Updating term to {}", *max_term);
                s.persisted.current_term = max(s.persisted.current_term, *max_term);
                s.persisted.voted_for = None;
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

/** Calculate the destination RPC url based on the id */
fn calculate_rpc_server_dst(id: &str) -> String {
    // For now keep it simple and assume it's a 0-based integer
    let id_int = id.parse::<u32>().unwrap();
    format!("http://[::1]:{}", 50000 + id_int)
}
