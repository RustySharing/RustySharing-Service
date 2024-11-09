use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;  // Ensure async-compatible Mutex
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use rand::Rng;
use serde::{Deserialize, Serialize};
use local_ip_address::local_ip;

#[derive(Debug, Clone, PartialEq)]
enum ServerState {
    Follower,
    Candidate,
    Leader,
}

struct RaftNode {
    state: ServerState,
    term: u64,
    voted_for: Option<String>,
    id: String,
    timeout_duration: Duration,
    last_heartbeat: Instant,
    peers: Vec<String>, // List of other servers' IP addresses
}

impl RaftNode {
    fn new(id: String, peers: Vec<String>) -> Self {
        RaftNode {
            state: ServerState::Follower,
            term: 0,
            voted_for: None,
            id,
            timeout_duration: Duration::from_millis(rand::thread_rng().gen_range(150..300)),
            last_heartbeat: Instant::now(),
            peers,
        }
    }

    async fn handle_election_timeout(&mut self) {
        if self.state == ServerState::Follower || self.state == ServerState::Candidate {
            if Instant::now().duration_since(self.last_heartbeat) > self.timeout_duration {
                println!("Election timeout, starting new election!");
                self.start_election().await;
            }
        }
    }

    async fn start_election(&mut self) {
        self.state = ServerState::Candidate;
        self.term += 1;
        self.voted_for = Some(self.id.clone());
        println!("Node {} is starting an election for term {}", self.id, self.term);

        let mut votes = 1; // Start with self-vote

        // Send vote requests to all peers
        for peer in &self.peers {
            match request_vote(peer, self.term, &self.id).await {
                Ok(vote_granted) => {
                    if vote_granted {
                        votes += 1;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to request vote from {}: {}", peer, e);
                }
            }
        }

        // If majority vote received, become leader
        if votes > self.peers.len() / 2 {
            println!("Node {} is elected as the leader!", self.id);
            self.state = ServerState::Leader;
        } else {
            println!("Node {} failed to become the leader", self.id);
            self.state = ServerState::Follower;
        }
    }
}

// Helper function to send vote requests over TCP
async fn request_vote(peer_ip: &str, term: u64, candidate_id: &str) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect(format!("{}:6000", peer_ip)).await?;
    let request = VoteRequest {
        term,
        candidate_id: candidate_id.to_string(),
    };
    let request_bytes = serde_json::to_vec(&request)?;
    stream.write_all(&request_bytes).await?;

    let mut buffer = [0; 128];
    let n = stream.read(&mut buffer).await?;
    let response: VoteResponse = serde_json::from_slice(&buffer[..n])?;
    Ok(response.vote_granted)
}

#[derive(Serialize, Deserialize)]
struct VoteRequest {
    term: u64,
    candidate_id: String,
}

#[derive(Serialize, Deserialize)]
struct VoteResponse {
    vote_granted: bool,
}

async fn start_vote_listener(raft: Arc<Mutex<RaftNode>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind("0.0.0.0:6000").await?;
    println!("Listening for vote requests...");

    loop {
        let (mut socket, _) = listener.accept().await?;
        let raft_clone = raft.clone();

        tokio::spawn(async move {
            let mut buffer = [0; 128];
            let n = socket.read(&mut buffer).await.expect("Failed to read data");
            let vote_request: VoteRequest = serde_json::from_slice(&buffer[..n]).expect("Failed to parse VoteRequest");

            let mut raft = raft_clone.lock().await;
            let vote_granted = if vote_request.term > raft.term {
                raft.term = vote_request.term;
                raft.voted_for = Some(vote_request.candidate_id.clone());
                true
            } else {
                false
            };

            let response = VoteResponse { vote_granted };
            let response_bytes = serde_json::to_vec(&response).expect("Failed to serialize VoteResponse");
            socket.write_all(&response_bytes).await.expect("Failed to send response");
        });
    }
}

async fn run_raft_election_checker(raft: Arc<Mutex<RaftNode>>) {
    loop {
        {
            let mut node = raft.lock().await;
            node.handle_election_timeout().await;
        }
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local_ip = local_ip().expect("Could not determine local IP address").to_string();
    let port = 6000;
    let id = format!("{}:{}", local_ip, port);

    let all_ips = vec![
        "10.7.17.128".to_string(),
        "10.7.16.11".to_string(),
        "10.7.16.54".to_string(),
    ];

    let peers: Vec<String> = all_ips.into_iter().filter(|ip| ip != &local_ip).collect();

    let raft_node = Arc::new(Mutex::new(RaftNode::new(id.clone(), peers)));
    let raft_node_clone = raft_node.clone();

    tokio::spawn(start_vote_listener(raft_node.clone()));
    tokio::spawn(run_raft_election_checker(raft_node_clone));

    println!("Server {} started", id);
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}
