use tonic::{transport::Server, Request, Response, Status};
use std::collections::HashMap;
use std::fs::{File, read};
use serde::{Deserialize, Serialize};
use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;

pub mod image_encoding {
    tonic::include_proto!("image_encoding");
}

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
        
        let vote_request = raft::VoteRequest {
            candidate_id: node.id.clone(),
            term: node.term as i32,
            last_log_index: 0,
            last_log_term: 0,
        };

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

#[derive(Default)]
pub struct MyService;

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

        let encoded_image = match encode_image(image_data.clone(), image_name) {
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        let encoded_bytes = std::fs::read(&encoded_image)?;

        let reply = image_encoding::EncodedImageResponse {
            image_data: encoded_bytes,
        };

        std::fs::remove_file(encoded_image)?;

        Ok(Response::new(reply))
    }
}

fn encode_image(image_data: Vec<u8>, image_name: &str) -> Result<String, std::io::Error> {
    let encoded_image_path = format!("/tmp/encoded_{}.png", image_name);
    std::fs::write(&encoded_image_path, image_data)?;
    Ok(encoded_image_path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;

    let node_id = format!("node-{}", ip.to_string());
    let mut node = RaftNode {
        id: node_id,
        state: RaftState::Candidate,
        term: 1,
        voted_for: None,
        peers: vec![
            "10.7.17.128:50051".to_string(),
            "10.17.16.11:50051".to_string(),
        ],
    };

    start_raft_election(&mut node).await;

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(image_encoding::image_encoder_server::ImageEncoderServer::new(MyService {}))
        .serve(addr)
        .await?;

    Ok(())
}
