use raft::raft_client::RaftClient;
use raft::AppendEntriesRequest;

pub mod raft {
    tonic::include_proto!("raft");
}

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let mut client = RaftClient::connect("http://[::1]:50051").await?;

//     let request = tonic::Request::new(AppendEntriesRequest { term: 1 });

//     let response = client.append_entries(request).await?;

//     println!("response={:?}", response);

//     Ok(())
// }
