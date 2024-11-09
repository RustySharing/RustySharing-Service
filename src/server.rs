use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};
use steganography::util::file_to_bytes;
use rand::Rng;
use local_ip_address::local_ip;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
// Import your encode_image function
use rpc_service::image_encoder::encode_image;

#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

// This module is generated from your .proto file
pub mod image_encoding {
    tonic::include_proto!("image_encoding");
}

pub mod leader_provider {
    tonic::include_proto!("leader_provider");
}

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
    encoding_socket: String,  // Socket for client-facing encoding service on port 50051
    timeout_duration: Duration,
    last_heartbeat: Instant,
    peers: Vec<String>,
    priority: u64,  // Each node knows only its own priority
}

impl RaftNode {
    fn new(id: String, peers: Vec<String>, encoding_socket: String, priority: u64) -> Self {
        RaftNode {
            state: ServerState::Follower,
            term: 0,
            voted_for: None,
            id,
            encoding_socket,
            timeout_duration: Duration::from_millis(rand::thread_rng().gen_range(150..300)),
            last_heartbeat: Instant::now(),
            peers,
            priority,
        }
    }

    async fn start_election(&mut self) -> String {
        self.state = ServerState::Candidate;
        self.term += 1;
        self.voted_for = Some(self.id.clone());
        println!("Node {} with priority {} is starting an election for term {}", self.id, self.priority, self.term);

        let mut votes = 1;

        for peer in &self.peers {
            match request_vote(peer, self.term, &self.id, self.priority).await {
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

        if votes > self.peers.len() / 2 {
            println!("Node {} is elected as the leader!", self.id);
            self.state = ServerState::Leader;
            return self.encoding_socket.clone(); // Return encoding socket on successful election
        } else {
            println!("Node {} failed to become the leader", self.id);
            self.state = ServerState::Follower;
            return "No leader elected".to_string();
        }
    }

    // Method to check if the current node is the leader
    fn is_leader(&self) -> bool {
        self.state == ServerState::Leader
    }
}

// Function to send a vote request to another peer
async fn request_vote(peer_ip: &str, term: u64, candidate_id: &str, candidate_priority: u64) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect(format!("{}:6000", peer_ip)).await?;
    let request = VoteRequest {
        term,
        candidate_id: candidate_id.to_string(),
        priority: candidate_priority,  // Include priority
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
    priority: u64,  // Add priority to the VoteRequest struct
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
            let vote_granted = if vote_request.term > raft.term || (vote_request.term == raft.term && vote_request.priority < raft.priority) {
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

// Define your ImageEncoderService
struct ImageEncoderService {
    raft: Arc<Mutex<RaftNode>>,
}

struct LeaderProviderService {
    raft: Arc<Mutex<RaftNode>>,
}

#[tonic::async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<leader_provider::LeaderProviderEmptyRequest>,
    ) -> Result<Response<leader_provider::LeaderProviderResponse>, Status> {
        let mut raft = self.raft.lock().await;
        let leader_socket = raft.start_election().await;

        let reply = leader_provider::LeaderProviderResponse {
            leader_socket,
        };
        Ok(Response::new(reply))
    }
}

#[tonic::async_trait]
impl ImageEncoder for ImageEncoderService {
    async fn image_encode(
        &self,
        request: Request<EncodedImageRequest>,
    ) -> Result<Response<EncodedImageResponse>, Status> {
        let raft = self.raft.lock().await;
        if !raft.is_leader() {
            return Err(Status::failed_precondition("This server is not the leader."));
        }

        let request = request.into_inner();
        println!("Got a request!");

        let image_data = &request.image_data;
        let image_name = &request.file_name;

        let encoded_image = match encode_image(image_data.clone(), image_name) {
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        let file = File::open(encoded_image.clone())?;
        let encoded_bytes = file_to_bytes(file);

        let reply = EncodedImageResponse {
            image_data: encoded_bytes.clone(),
        };

        std::fs::remove_file(encoded_image)?;

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let local_ip = local_ip().expect("Could not determine local IP address").to_string();
    let id = format!("{}:6000", local_ip);

    // Set the client-facing encoding service socket on port 50051
    let encoding_socket = format!("{}:50051", local_ip);

    // Each node only knows its own priority
    let priority = rand::thread_rng().gen_range(1..100);

    let all_ips = vec![
        "10.7.17.128".to_string(),
        "10.7.16.11".to_string(),
    ];

    let peers: Vec<String> = all_ips.into_iter().filter(|ip| ip != &local_ip).collect();

    let raft_node = Arc::new(Mutex::new(RaftNode::new(id.clone(), peers, encoding_socket.clone(), priority)));
    tokio::spawn(start_vote_listener(raft_node.clone()));

    let addr = encoding_socket.parse()?;
    let image_encoder_service = ImageEncoderService {
        raft: raft_node.clone(),
    };
    let leader_provider_service = LeaderProviderService {
        raft: raft_node.clone(),
    };

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .serve(addr)
        .await?;

    Ok(())
}
