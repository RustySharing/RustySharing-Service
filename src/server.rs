use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use steganography::util::file_to_bytes;
use tokio::sync::mpsc;
use tonic::{transport::Server, Request, Response, Status};
use uuid::Uuid;
use almost_raft::{election::{RaftElectionState, raft_election}, Message, Node};
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use tokio::time::sleep;

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

// Define a struct representing a node in the cluster
#[derive(Debug, Clone)]
struct ClusterNode {
    id: String,
    sender: Sender<Message<ClusterNode>>,
}

#[async_trait]
impl Node for ClusterNode {
    type NodeType = ClusterNode;

    async fn send_message(&self, msg: Message<Self::NodeType>) {
        if let Err(e) = self.sender.send(msg).await {
            eprintln!("Failed to send message: {}", e);
        }
    }

    fn node_id(&self) -> &String {
        &self.id
    }
}

// Define your ImageEncoderService
struct ImageEncoderService {
    is_leader: Arc<Mutex<bool>>,
}

#[tonic::async_trait]
impl ImageEncoder for ImageEncoderService {
    async fn image_encode(
        &self,
        request: Request<EncodedImageRequest>,
    ) -> Result<Response<EncodedImageResponse>, Status> {
        // Check if this node is the leader
        {
            let is_leader = self.is_leader.lock().unwrap();
            if !*is_leader {
                return Err(Status::failed_precondition("This node is not the leader"));
            }
        }

        let request = request.into_inner();
        println!("Got a request!");

        // Get the image data from the request
        let image_data = &request.image_data; // Assuming image_data is passed as bytes
        let image_name = &request.file_name;

        // Call the encode_image function with the provided image data
        let encoded_image = match encode_image(image_data.clone(), image_name) {
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        let file = File::open(&encoded_image)?;
        let encoded_bytes = file_to_bytes(file);
        // Construct the response with the encoded image data
        let reply = EncodedImageResponse {
            image_data: encoded_bytes.clone(),
        };

        // Delete the file when done
        std::fs::remove_file(encoded_image)?;

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let node_ips = vec![
        "10.7.16.11".to_string(),
        "10.7.17.128".to_string(),
        "10.7.16.54".to_string(),
    ];

    let self_id = Uuid::new_v4().to_string();
    let (tx, mut from_raft) = mpsc::channel(100); // Increased channel buffer capacity

    println!("Initializing Raft election state for node {}", self_id);
    let (state, tx_to_raft) = RaftElectionState::init(
        self_id.clone(),
        5000,
        1000,
        20,
        vec![],
        tx.clone(),
        3,
        2,
    );

    tokio::spawn(raft_election(state));

    let is_leader = Arc::new(Mutex::new(false));
    let is_leader_clone = Arc::clone(&is_leader);

    tokio::spawn(async move {
        while let Some(message) = from_raft.recv().await {
            match message {
                Message::ControlLeaderChanged(leader_id) => {
                    let mut is_leader = is_leader_clone.lock().unwrap();
                    *is_leader = leader_id == self_id;
                    if *is_leader {
                        println!("Node {} is now the leader", self_id);
                    } else {
                        println!("Node {} is a follower. Leader ID: {}", self_id, leader_id);
                    }
                }
                _ => {}
            }
        }
    });

    // Delay node addition to give the election time to initialize
    tokio::time::sleep(Duration::from_secs(2)).await;

    for ip in &node_ips {
        let node_id = Uuid::new_v4().to_string();
        let (node_tx, _node_rx) = mpsc::channel(10);
        let node = ClusterNode {
            id: node_id.clone(),
            sender: node_tx,
        };
        println!("Adding node {} with IP {} to the Raft cluster", node_id, ip);

        if let Err(e) = tx_to_raft.send(Message::ControlAddNode(node)).await {
            eprintln!("Failed to add node {}: {}", node_id, e);
        }
    }

    sleep(Duration::from_secs(6)).await;

    let addr = format!("{}:50051", local_ip::get().unwrap()).parse()?;
    let image_encoder_service = ImageEncoderService { is_leader };

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .serve(addr)
        .await?;

    Ok(())
}

