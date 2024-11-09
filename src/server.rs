use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use leader_provider::{LeaderProviderEmptyRequest, LeaderProviderResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{self, Sender};
use tokio::task;
use tonic::{transport::Server, Request, Response, Status};
use uuid::Uuid;
use almost_raft::{election::{raft_election, RaftElectionState}, Message, Node};
use async_trait::async_trait;
use tokio::fs;

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

// Define your ImageEncoderService
struct ImageEncoderService {}

#[tonic::async_trait]
impl ImageEncoder for ImageEncoderService {
    async fn image_encode(
        &self,
        request: Request<EncodedImageRequest>,
    ) -> Result<Response<EncodedImageResponse>, Status> {
        let request = request.into_inner();
        println!("Got a request!");

        let image_data = &request.image_data; // Assuming image_data is passed as bytes

        let encoded_image = match encode_image(image_data.clone()) {
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        let encoded_bytes = fs::read(encoded_image)
            .await
            .map_err(|_| Status::internal("Failed to read encoded image file"))?;

        let reply = EncodedImageResponse {
            image_data: encoded_bytes,
        };

        Ok(Response::new(reply))
    }
}

// Define the NodeMPSC struct
#[derive(Debug, Clone)]
struct NodeMPSC {
    id: String,
    sender: Sender<Message<NodeMPSC>>,
    address: String,
}

#[async_trait]
impl Node for NodeMPSC {
    type NodeType = NodeMPSC;

    async fn send_message(&self, msg: Message<Self::NodeType>) {
        if let Err(e) = self.sender.send(msg).await {
            eprintln!("Failed to send message: {}", e);
        }
    }

    fn node_id(&self) -> &String {
        &self.id
    }
}

// Define the LeaderProviderService
struct LeaderProviderService {
    nodes: Arc<Mutex<HashMap<String, NodeMPSC>>>,
    current_leader: Arc<Mutex<Option<String>>>, // Track the current leader
}

#[tonic::async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<LeaderProviderEmptyRequest>,
    ) -> Result<Response<LeaderProviderResponse>, Status> {
        println!("Got a request for the current leader!");

        let current_leader = self.current_leader.lock().unwrap();
        if let Some(leader_id) = &*current_leader {
            let nodes = self.nodes.lock().unwrap();
            if let Some(leader_node) = nodes.get(leader_id) {
                let reply = LeaderProviderResponse {
                    leader_socket: leader_node.address.clone(),
                };
                return Ok(Response::new(reply));
            }
        }

        Err(Status::not_found("Leader not found"))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let node_addresses = vec![
        "10.7.17.128:50051".to_string(),
        "10.7.16.11:50051".to_string(),
        "10.7.16.54:50051".to_string(),
    ];

    let mut node_senders = HashMap::new();
    let mut node_receivers = HashMap::new();

    for address in &node_addresses {
        let (tx, rx) = mpsc::channel(100);
        node_senders.insert(address.clone(), tx);
        node_receivers.insert(address.clone(), rx);
    }

    let mut nodes = HashMap::new();
    for address in &node_addresses {
        let id = Uuid::new_v4().to_string();
        let sender = node_senders.get(address).unwrap().clone();
        let node = NodeMPSC {
            id: id.clone(),
            sender,
            address: address.clone(),
        };
        nodes.insert(id, node);
    }

    let nodes_arc = Arc::new(Mutex::new(nodes));
    let current_leader = Arc::new(Mutex::new(None)); // Initially, no leader

    let self_id = Uuid::new_v4().to_string();
    let timeout = 5000;
    let heartbeat_interval = 1000;
    let message_timeout = 20;
    let max_node = 5;
    let min_node = 3;

    let (tx_to_raft, from_raft) = mpsc::channel(100);

    // Initialize RaftElectionState and unwrap the Arc to move it into the task
    let (election_state, _) = RaftElectionState::init(
        self_id.clone(),
        timeout,
        heartbeat_interval,
        message_timeout,
        vec![],
        tx_to_raft.clone(),
        max_node,
        min_node,
    );

    let current_leader_clone = current_leader.clone();

    // Spawn the Raft election process
    task::spawn(async move {
        let mut from_raft = from_raft;

        // Listen for messages from Raft
        while let Some(msg) = from_raft.recv().await {
            match msg {
                Message::ControlLeaderChanged(leader_id) => {
                    println!("Leader changed to {}", leader_id);
                    let mut leader_lock = current_leader_clone.lock().unwrap();
                    *leader_lock = Some(leader_id);
                }
                _ => {}
            }
        }

        // Start the Raft election directly
        raft_election(election_state).await;
    });

    // Add nodes to the election state
    for node in nodes_arc.lock().unwrap().values() {
        let msg = Message::ControlAddNode(node.clone());
        tx_to_raft.send(msg).await.expect("Failed to add node to the election state");
    }

    let addr = "[::1]:50051".parse()?;
    let image_encoder_service = ImageEncoderService {};
    let leader_provider_service = LeaderProviderService {
        nodes: nodes_arc.clone(),
        current_leader: current_leader.clone(),
    };

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .serve(addr)
        .await?;

    Ok(())
}
