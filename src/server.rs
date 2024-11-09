use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use steganography::util::file_to_bytes;
use sysinfo::System;
use tokio::sync::Mutex;
// use tokio::time::{sleep, Duration};
use tonic::{transport::Server, Request, Response, Status};
// Import your encode_image function
use rpc_service::image_encoder::encode_image;

pub mod node_communication {
    tonic::include_proto!("node_communication");
}

// This module is generated from your .proto file
pub mod image_encoding {
    tonic::include_proto!("image_encoding");
}

pub mod leader_provider {
    tonic::include_proto!("leader_provider");
}

use node_communication::node_communicator_client::NodeCommunicatorClient;
use node_communication::node_communicator_server::{NodeCommunicator, NodeCommunicatorServer};
use node_communication::{LoadUpdate, LoadUpdateResponse};

#[derive(Clone, Debug)]
struct NodeInfo {
    address: String,
    load: f32,
    last_updated: u64,
    rank: u64,
}

#[derive(Debug)]
struct LeaderElection {
    nodes: HashMap<String, NodeInfo>,
    current_leader: Option<String>,
    self_address: String,
    known_peers: Vec<String>,
    system: System,
}

struct LeaderProviderService {
    state: LeaderState,
}

struct ImageEncoderService {}

#[derive(Clone)]
struct LeaderState {
    election: Arc<Mutex<LeaderElection>>,
}

// New service for inter-node communication
struct NodeCommunicationService {
    state: LeaderState,
}

#[tonic::async_trait]
impl NodeCommunicator for NodeCommunicationService {
    async fn update_load(
        &self,
        request: Request<LoadUpdate>,
    ) -> Result<Response<LoadUpdateResponse>, Status> {
        let update = request.into_inner();
        let mut election = self.state.election.lock().await;

        election.update_node(update.node_address, update.load);

        Ok(Response::new(LoadUpdateResponse {}))
    }
}

impl LeaderElection {
    fn new(self_address: String, known_peers: Vec<String>) -> Self {
        LeaderElection {
            nodes: HashMap::new(),
            current_leader: None,
            self_address,
            known_peers,
            system: System::new_all(),
        }
    }

    fn get_current_load(&mut self) -> f32 {
        self.system.refresh_cpu_usage();
        let cpus = self.system.cpus();
        let mut sum_cpu: f32 = 0.0;
        for cpu in cpus {
            sum_cpu += cpu.cpu_usage();
        }
        let cpu_load = sum_cpu / cpus.len() as f32;
        self.system.refresh_memory();
        let memory_load =
            (self.system.used_memory() as f32 / self.system.total_memory() as f32) * 100.0;

        0.7 * cpu_load + 0.3 * memory_load
    }

    fn update_node(&mut self, address: String, load: f32) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let rank = (load * 1000.0) as u64 + timestamp % 1000;

        self.nodes.insert(
            address.clone(),
            NodeInfo {
                address: address.clone(),
                load,
                last_updated: timestamp,
                rank,
            },
        );
    }

    async fn broadcast_load(
        &self,
        load: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for peer in &self.known_peers {
            if peer != &self.self_address {
                let mut client =
                    NodeCommunicatorClient::connect(format!("http://{}", peer)).await?;

                let request = Request::new(LoadUpdate {
                    node_address: self.self_address.clone(),
                    load,
                });

                client.update_load(request).await?;
            }
        }
        Ok(())
    }

    fn elect_leader(&mut self) -> Option<String> {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.nodes
            .retain(|_, info| current_time - info.last_updated < 10);

        self.nodes
            .values()
            .min_by(|a, b| a.load.partial_cmp(&b.load).unwrap())
            .map(|node| node.address.clone())
    }
}

impl LeaderProviderService {
    pub fn new(self_address: String, known_peers: Vec<String>) -> Self {
        let election = LeaderElection::new(self_address, known_peers);
        LeaderProviderService {
            state: LeaderState {
                election: Arc::new(Mutex::new(election)),
            },
        }
    }
}

impl NodeCommunicationService {
    pub fn new(state: LeaderState) -> Self {
        NodeCommunicationService { state }
    }
}

// Define your ImageEncoderService

#[tonic::async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<leader_provider::LeaderProviderEmptyRequest>,
    ) -> Result<Response<leader_provider::LeaderProviderResponse>, Status> {
        println!("Got a request for a leader provider!");

        let mut election = self.state.election.lock().await;

        // Get current load
        let current_load = election.get_current_load();
        let self_address = election.self_address.clone();

        // Update own node info
        election.update_node(self_address.clone(), current_load);

        // Broadcast load to other nodes
        if let Err(e) = election.broadcast_load(current_load).await {
            eprintln!("Failed to broadcast load: {}", e);
        }

        // Perform election
        let leader_address = match election.elect_leader() {
            Some(leader) => leader,
            None => return Err(Status::internal("No available leader")),
        };

        let reply = leader_provider::LeaderProviderResponse {
            leader_socket: leader_address,
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
        let request = request.into_inner();
        println!("Got a request!");

        // Get the image data from the request
        let image_data = &request.image_data; // Assuming image_data is passed as bytes
        let image_name = &request.file_name;

        // Step 1: Load the image from the byte data
        // let img = image::load_from_memory(image_data)
        //     .map_err(|_| Status::internal("Failed to load image from memory"))?;

        // Call the encode_image function with the loaded image
        let encoded_image = match encode_image(image_data.clone(), image_name) {
            // Pass the loaded image directly
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        let file = File::open(encoded_image.clone())?;
        let encoded_bytes = file_to_bytes(file);
        // Construct the response with the encoded image data
        let reply = EncodedImageResponse {
            image_data: encoded_bytes.clone(), // Echo the original image data in the response
        };

        // delete file file when done
        std::fs::remove_file(encoded_image)?;

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let port = 50051; // You might want to make this configurable
    let addr = format!("{}:{}", ip.to_string(), port).parse()?;
    let self_address = format!("{}:{}", ip.to_string(), port);

    // List of known peers (you'll need to configure this)
    let known_peers = vec![
        "192.168.1.1:50051".to_string(),
        "192.168.1.2:50051".to_string(),
        "192.168.1.3:50051".to_string(),
    ];

    let image_encoder_service = ImageEncoderService {};
    let leader_provider_service = LeaderProviderService::new(self_address.clone(), known_peers);

    // Share state between services
    let node_communication_service =
        NodeCommunicationService::new(leader_provider_service.state.clone());

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .add_service(NodeCommunicatorServer::new(node_communication_service))
        .serve(addr)
        .await?;

    Ok(())
}
