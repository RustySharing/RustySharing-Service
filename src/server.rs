use tonic::{transport::Server, Request, Response, Status};
use std::collections::HashMap;
use std::fs::{File, read};
use serde::{Deserialize, Serialize};
use rand::Rng;  // For generating random numbers (e.g., for simulating votes)

pub mod image_encoding {
    tonic::include_proto!("image_encoding");  // Include generated proto code
}

pub mod raft {
    tonic::include_proto!("raft");  // Include generated proto code for raft
}

// Define the RaftNode struct
#[derive(Debug, Clone)]  // Derive Clone here for RaftNode
pub struct RaftNode {
    pub id: String,
    pub state: RaftState,
    pub term: u32,
    pub voted_for: Option<String>,
    pub peers: Vec<String>,
}

// Raft state enum
#[derive(Clone, Debug, PartialEq)]  // Add Clone and PartialEq
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}

// Simulated Raft leader election
async fn start_raft_election(node: &RaftNode) {
    let mut votes_received: HashMap<String, bool> = HashMap::new();
    let peers = &node.peers;
    
    let vote_request = raft::VoteRequest {
        candidate_id: node.id.clone(),
        term: node.term as i32,  // Casting to i32
        last_log_index: 0,
        last_log_term: 0,
    };

    // Simulate leader election process
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
    } else {
        println!("Election failed for candidate: {}", node.id);
    }
}

// Define the MyService struct
#[derive(Default)]
pub struct MyService;

// Implement the ImageEncoder trait for the MyService struct
#[tonic::async_trait]
impl image_encoding::image_encoder_server::ImageEncoder for MyService {
    async fn image_encode(
        &self,
        request: Request<image_encoding::EncodedImageRequest>,
    ) -> Result<Response<image_encoding::EncodedImageResponse>, Status> {
        let request = request.into_inner();
        println!("Got a request to encode image!");

        let image_data = &request.image_data;
        let image_name = &request.file_name;

        // Call the encode_image function to encode the image
        let encoded_image = match encode_image(image_data.clone(), image_name) {
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        // Read the encoded file and return the response
        let encoded_bytes = std::fs::read(&encoded_image)?;  // Use path for reading file

        let reply = image_encoding::EncodedImageResponse {
            image_data: encoded_bytes,
        };

        // Optionally delete the file when done
        std::fs::remove_file(encoded_image)?;

        Ok(Response::new(reply))
    }
}

// Dummy encode_image function (replace with your actual implementation)
fn encode_image(image_data: Vec<u8>, image_name: &str) -> Result<String, std::io::Error> {
    let encoded_image_path = format!("/tmp/encoded_{}.png", image_name);
    // Simulate encoding (write the image data to a file)
    std::fs::write(&encoded_image_path, image_data)?;
    Ok(encoded_image_path)
}

// Main function to start the server
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;

    // Example of RaftNode
    let node = RaftNode {
        id: "node-1".to_string(),
        state: RaftState::Candidate,
        term: 1,
        voted_for: None,
        peers: vec!["node-2".to_string(), "node-3".to_string()],
    };

    start_raft_election(&node).await;

    // Add the image encoding service to the gRPC server
    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .add_service(image_encoding::image_encoder_server::ImageEncoderServer::new(MyService {}))
        .serve(addr)
        .await?;

    Ok(())
}
