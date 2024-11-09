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
    // Define the IP addresses of the nodes in the cluster
    let node_ips = vec![
        "10.7.16.11".to_string(),
        "10.7.17.128".to_string(),
        "10.7.16.54".to_string(),
    ];

    // Initialize the node's unique ID
    let self_id = Uuid::new_v4().to_string();

    // Create a channel for communication with the Raft election process
    let (tx, mut from_raft) = mpsc::channel(10);

    // Initialize the Raft election state
    let (state, tx_to_raft) = RaftElectionState::init(
        self_id.clone(),
        5000,   // timeout in milliseconds
        1000,   // heartbeat interval in milliseconds
        20,     // message timeout in milliseconds
        vec![], // initial list of nodes; can be populated later
        tx.clone(),
        3,      // number of nodes in the cluster
        2,      // min number of nodes to start election
    );

    // Spawn the Raft election process
    tokio::spawn(raft_election(state));

    // Shared state to track if this node is the leader
    let is_leader = Arc::new(Mutex::new(false));
    let is_leader_clone = Arc::clone(&is_leader);

    // Spawn a task to listen for leadership changes
    tokio::spawn(async move {
        while let Some(message) = from_raft.recv().await {
            match message {
                Message::ControlLeaderChanged(leader_id) => { // Updated to use ControlLeaderChanged
                    let mut is_leader = is_leader_clone.lock().unwrap();
                    *is_leader = leader_id == self_id;
                    if *is_leader {
                        println!("This node is now the leader");
                    } else {
                        println!("This node is a follower. Leader ID: {}", leader_id);
                    }
                }
                _ => {}
            }
        }
    });

    // Add each node to the cluster
    for ip in &node_ips {
        let node_id = Uuid::new_v4().to_string();
        let (node_tx, _node_rx) = mpsc::channel(10);
        let node = ClusterNode {
            id: node_id.clone(),
            sender: node_tx,
        };
        tx_to_raft
            .send(Message::ControlAddNode(node))
            .await
            .expect("Failed to add node");
    }

    // Wait for the election to complete
    sleep(Duration::from_secs(6)).await;

    // Start the gRPC server
    let addr = format!("{}:50051", local_ip::get().unwrap()).parse()?;
    let image_encoder_service = ImageEncoderService { is_leader };

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .serve(addr)
        .await?;

    Ok(())
}
