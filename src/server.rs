use tonic::{transport::Server, Request, Response, Status};
use std::collections::HashMap;
use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;
use std::sync::{Arc, Mutex};

pub mod raft {
    tonic::include_proto!("raft");
}

#[derive(Debug, Clone)]
pub struct RaftNode {
    pub id: String,
    pub state: RaftState,
    pub term: u32,
    pub voted_for: Option<String>,
    pub peers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}

async fn start_raft_election(node: &mut RaftNode) {
    loop {
        node.term += 1;
        node.state = RaftState::Candidate;
        node.voted_for = Some(node.id.clone());

        let mut votes_received: HashMap<String, bool> = HashMap::new();
        let peers = &node.peers;

        for peer in peers {
            if rand::random::<f32>() < 0.5 {
                votes_received.insert(peer.clone(), true);
            } else {
                votes_received.insert(peer.clone(), false);
            }
        }

        let votes_granted = votes_received.values().filter(|&&v| v).count();
        
        if votes_granted > peers.len() / 2 {
            println!("Leader elected: {}", node.id);
            node.state = RaftState::Leader;
            break;
        } else {
            println!("Election failed for candidate: {}", node.id);
            node.state = RaftState::Follower;
            node.voted_for = None;

            let retry_delay = rand::thread_rng().gen_range(150..300);
            sleep(Duration::from_millis(retry_delay)).await;
        }
    }
}

pub struct RaftServiceImpl {
    pub node: Arc<Mutex<RaftNode>>, // Use Arc<Mutex<T>> for internal mutability
}

#[tonic::async_trait]
impl raft::raft_service_server::RaftService for RaftServiceImpl {
    async fn elect_leader(
        &self,
        _request: Request<raft::ElectLeaderRequest>,
    ) -> Result<Response<raft::ElectLeaderResponse>, Status> {
        let mut node = self.node.lock().unwrap().clone(); // Lock and clone the node for mutation
        start_raft_election(&mut node).await;

        Ok(Response::new(raft::ElectLeaderResponse {
            message: format!("Leader elected: {}", node.id),
        }))
    }

    async fn request_vote(
        &self,  // Keep &self
        request: Request<raft::VoteRequest>,
    ) -> Result<Response<raft::VoteResponse>, Status> {
        let request = request.into_inner();
        println!("Received vote request from candidate: {}", request.candidate_id);

        // Lock the node to modify it safely
        let mut node = self.node.lock().unwrap();
        
        // Simple voting logic: approve vote if no current vote or if term is newer
        let vote_granted = if node.voted_for.is_none() || request.term > node.term as i32 {
            node.voted_for = Some(request.candidate_id.clone());
            true
        } else {
            false
        };

        Ok(Response::new(raft::VoteResponse {
            vote_granted,
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;

    let node_id = format!("node-{}", ip.to_string());
    let node = RaftNode {
        id: node_id,
        state: RaftState::Follower,
        term: 1,
        voted_for: None,
        peers: vec![
            "10.7.17.128:50051".to_string(),
            "10.17.16.11:50051".to_string(),
        ],
    };

    let raft_service = RaftServiceImpl { 
        node: Arc::new(Mutex::new(node)) // Wrap node in Arc<Mutex<T>> here
    };

    Server::builder()
        .add_service(raft::raft_service_server::RaftServiceServer::new(raft_service))
        .serve(addr)
        .await?;

    Ok(())
}
