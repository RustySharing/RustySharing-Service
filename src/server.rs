use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tonic::{transport::Server, Request, Response, Status};
use steganography::util::file_to_bytes;
use local_ip_address::local_ip;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use rpc_service::image_encoder::encode_image;

#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

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
    encoding_socket: String,
    timeout_duration: Duration,
    last_heartbeat: Instant,
    peers: Vec<String>,
    load: u64,
    load_table: HashMap<String, u64>,
    current_leader: Option<String>,
}

impl RaftNode {
    fn new(id: String, peers: Vec<String>, encoding_socket: String, load: u64) -> Self {
        let mut load_table = HashMap::new();
        for peer in &peers {
            load_table.insert(peer.clone(), u64::MAX);
        }
        
        RaftNode {
            state: ServerState::Follower,
            term: 0,
            voted_for: None,
            id,
            encoding_socket,
            timeout_duration: Duration::from_millis(200),
            last_heartbeat: Instant::now(),
            peers,
            load,
            load_table,
            current_leader: None,
        }
    }

    async fn start_election(&mut self) -> String {
        self.state = ServerState::Candidate;
        self.term += 1;
        self.voted_for = Some(self.id.clone());

        println!("Node {} with load {} is starting an election for term {}", self.id, self.load, self.term);

        self.load_table.insert(self.id.clone(), self.load);

        for peer in &self.peers {
            match request_vote(peer, self.term, &self.id, self.load).await {
                Ok((vote_granted, peer_load, peer_socket)) => {
                    self.load_table.insert(peer.clone(), peer_load);

                    if vote_granted && peer_load < self.load {
                        println!("Node {} is giving up leadership to {} with lower load {}", self.id, peer_socket, peer_load);
                        self.state = ServerState::Follower;
                        let _ = confirm_leadership(&peer_socket).await;
                        return peer_socket;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to request vote from {}: {}", peer, e);
                }
            }
        }

        println!("Node {} Load Table After Election: {:?}", self.id, self.load_table);

        println!("Node {} is elected as the leader with load {}", self.id, self.load);
        self.state = ServerState::Leader;
        self.current_leader = Some(self.encoding_socket.clone());

        for peer in &self.peers {
            let _ = notify_leader(peer, &self.encoding_socket).await;
        }

        self.encoding_socket.clone()
    }

    fn is_leader(&self) -> bool {
        self.state == ServerState::Leader
    }
}

// Function to send a vote request to another peer, including the candidate's load
async fn request_vote(
    peer_ip: &str,
    term: u64,
    candidate_id: &str,
    candidate_load: u64,
) -> Result<(bool, u64, String), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect(format!("{}:6000", peer_ip)).await?;
    let request = VoteRequest {
        term,
        candidate_id: candidate_id.to_string(),
        candidate_load,
    };
    let request_bytes = serde_json::to_vec(&request)?;
    stream.write_all(&request_bytes).await?;

    let mut buffer = [0; 128];
    let n = stream.read(&mut buffer).await?;
    let response: VoteResponse = serde_json::from_slice(&buffer[..n])?;
    Ok((response.vote_granted, response.node_load, response.encoding_socket))
}

// Function to confirm a new leader
async fn confirm_leadership(leader_socket: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect(format!("{}:6000", leader_socket)).await?;
    let confirmation = LeaderConfirmation { is_leader: true };
    let confirmation_bytes = serde_json::to_vec(&confirmation)?;
    stream.write_all(&confirmation_bytes).await?;
    Ok(())
}

// Function to notify peers of the new leader
async fn notify_leader(peer_ip: &str, leader_socket: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = TcpStream::connect(format!("{}:6000", peer_ip)).await?;
    let notification = LeaderNotification {
        leader_socket: leader_socket.to_string(),
    };
    let notification_bytes = serde_json::to_vec(&notification)?;
    stream.write_all(&notification_bytes).await?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct VoteRequest {
    term: u64,
    candidate_id: String,
    candidate_load: u64,
}

#[derive(Serialize, Deserialize)]
struct VoteResponse {
    vote_granted: bool,
    node_load: u64,
    encoding_socket: String,
}

#[derive(Serialize, Deserialize)]
struct LeaderNotification {
    leader_socket: String,
}

#[derive(Serialize, Deserialize)]
struct LeaderConfirmation {
    is_leader: bool,
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

            if let Ok(vote_request) = serde_json::from_slice::<VoteRequest>(&buffer[..n]) {
                let mut raft = raft_clone.lock().await;
                let vote_granted = if vote_request.term > raft.term {
                    raft.term = vote_request.term;
                    raft.voted_for = Some(vote_request.candidate_id.clone());
                    true
                } else if vote_request.term == raft.term && vote_request.candidate_load < raft.load {
                    raft.voted_for = Some(vote_request.candidate_id.clone());
                    true
                } else {
                    false
                };

                let response = VoteResponse {
                    vote_granted,
                    node_load: raft.load,
                    encoding_socket: raft.encoding_socket.clone(),
                };
                let response_bytes = serde_json::to_vec(&response).expect("Failed to serialize VoteResponse");
                socket.write_all(&response_bytes).await.expect("Failed to send response");
            } else if let Ok(leader_notification) = serde_json::from_slice::<LeaderNotification>(&buffer[..n]) {
                let mut raft = raft_clone.lock().await;
                raft.current_leader = Some(leader_notification.leader_socket.clone());
                raft.state = ServerState::Follower;
                println!("Node {} acknowledged new leader at {}", raft.id, leader_notification.leader_socket);
            } else if let Ok(leader_confirmation) = serde_json::from_slice::<LeaderConfirmation>(&buffer[..n]) {
                let mut raft = raft_clone.lock().await;
                if leader_confirmation.is_leader {
                    raft.state = ServerState::Leader;
                    raft.current_leader = Some(raft.encoding_socket.clone());
                    println!("Node {} is now confirmed as leader", raft.id);
                }
            }
        });
    }
}

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

    let encoding_socket = format!("{}:50051", local_ip);
    let all_ips = vec!["10.7.17.128".to_string(), "10.7.16.11".to_string(), "10.7.16.54".to_string()];
    let peers: Vec<String> = all_ips.into_iter().filter(|ip| ip != &local_ip).collect();

    let load = match local_ip.as_str() {
        "10.7.17.128" => 15,
        "10.7.16.11" => 20,
        "10.7.16.54" => 10,
        _ => 100,
    };

    let raft_node = Arc::new(Mutex::new(RaftNode::new(id.clone(), peers, encoding_socket.clone(), load)));
    tokio::spawn(start_vote_listener(raft_node.clone()));

    let addr = encoding_socket.parse()?;
    let image_encoder_service = ImageEncoderService { raft: raft_node.clone() };
    let leader_provider_service = LeaderProviderService { raft: raft_node.clone() };

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .serve(addr)
        .await?;

    Ok(())
}
