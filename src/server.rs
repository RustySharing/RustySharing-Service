use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use rand::Rng;
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use steganography::util::file_to_bytes;
use sysinfo::System;
use tokio::sync::Mutex;
use tokio::time::interval;
use tokio::time::Duration;
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
    self_rank: u64,
}

#[derive(Clone)]
struct LeaderState {
    election: Arc<Mutex<LeaderElection>>,
}
struct LeaderProviderService {
    state: LeaderState,
}

struct NodeCommunicationService {
    state: LeaderState,
}

struct ImageEncoderService {}

// New service for inter-node communication

#[tonic::async_trait]
impl NodeCommunicator for NodeCommunicationService {
    async fn update_load(
        &self,
        request: Request<LoadUpdate>,
    ) -> Result<Response<LoadUpdateResponse>, Status> {
        let update = request.into_inner();
        println!(
            "DEBUG: Received load update from {}: load={}, rank={}",
            update.node_address, update.load, update.rank
        );

        let mut election = self.state.election.lock().await;
        election.update_node(update.node_address, update.load, update.rank);

        Ok(Response::new(LoadUpdateResponse {}))
    }
}

impl LeaderElection {
    fn new(self_address: String, known_peers: Vec<String>) -> Self {
        let self_rank = rand::thread_rng().gen_range(0..u64::MAX);
        println!(
            "DEBUG: Initializing node {} with rank {}",
            self_address, self_rank
        );
        println!("DEBUG: Known peers: {:?}", known_peers);

        LeaderElection {
            nodes: HashMap::new(),
            current_leader: None,
            self_address,
            known_peers,
            system: System::new_all(),
            self_rank,
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

    fn update_node(&mut self, address: String, load: f32, rank: u64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        println!(
            "DEBUG: Updating node {} with load {} and rank {}",
            address, load, rank
        );

        self.nodes.insert(
            address.clone(),
            NodeInfo {
                address: address.clone(),
                load,
                last_updated: timestamp,
                rank,
            },
        );

        println!("DEBUG: Current nodes state after update:");
        for (addr, info) in &self.nodes {
            println!(
                "DEBUG: Node {}: load={}, rank={}, last_updated={}",
                addr, info.load, info.rank, info.last_updated
            );
        }
    }

    async fn broadcast_load(
        &self,
        load: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!(
            "DEBUG: Broadcasting load {} from node {}",
            load, self.self_address
        );

        for peer in &self.known_peers {
            if peer != &self.self_address {
                println!("DEBUG: Sending load update to peer {}", peer);

                match NodeCommunicatorClient::connect(format!("http://{}", peer)).await {
                    Ok(mut client) => {
                        let request = Request::new(LoadUpdate {
                            node_address: self.self_address.clone(),
                            load,
                            rank: self.self_rank,
                        });

                        match client.update_load(request).await {
                            Ok(_) => println!("DEBUG: Successfully sent load update to {}", peer),
                            Err(e) => {
                                println!("DEBUG: Failed to send load update to {}: {}", peer, e)
                            }
                        }
                    }
                    Err(e) => println!("DEBUG: Failed to connect to peer {}: {}", peer, e),
                }
            }
        }
        Ok(())
    }

    fn elect_leader(&mut self) -> Option<String> {
        println!("\nDEBUG: Starting leader election process");
        println!("DEBUG: Current nodes before cleanup:");
        for (addr, info) in &self.nodes {
            println!(
                "DEBUG: Node {}: load={}, rank={}, last_updated={}",
                addr, info.load, info.rank, info.last_updated
            );
        }

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Remove stale nodes
        let stale_threshold = 10; // 10 seconds
        self.nodes.retain(|addr, info| {
            let is_fresh = current_time - info.last_updated < stale_threshold;
            if !is_fresh {
                println!("DEBUG: Removing stale node {}", addr);
            }
            is_fresh
        });

        println!("DEBUG: Nodes after cleanup:");
        for (addr, info) in &self.nodes {
            println!(
                "DEBUG: Node {}: load={}, rank={}, last_updated={}",
                addr, info.load, info.rank, info.last_updated
            );
        }

        // Add self to the nodes if not already present
        if !self.nodes.contains_key(&self.self_address) {
            println!("DEBUG: Adding self to nodes list");
            let current_load = self.get_current_load();
            self.update_node(self.self_address.clone(), current_load, self.self_rank);
        }

        const LOAD_THRESHOLD: f32 = 10.0;

        // Find node with minimum load
        let min_load_node = self
            .nodes
            .values()
            .min_by(|a, b| {
                let comp = a.load.partial_cmp(&b.load).unwrap();
                println!(
                    "DEBUG: Comparing nodes {} (load={}, rank={}) and {} (load={}, rank={})",
                    a.address, a.load, a.rank, b.address, b.load, b.rank
                );
                match comp {
                    std::cmp::Ordering::Equal => {
                        println!("DEBUG: Loads are equal, comparing ranks");
                        b.rank.cmp(&a.rank) // Higher rank wins
                    }
                    other => {
                        println!("DEBUG: Loads are different, selecting lower load");
                        other
                    }
                }
            })
            .map(|node| node.address.clone());

        // Leadership stability logic
        if let Some(current_leader) = self.current_leader.as_ref() {
            println!("DEBUG: Current leader is {}", current_leader);
            if let (Some(current_info), Some(min_load_info)) = (
                self.nodes.get(current_leader),
                min_load_node.as_ref().and_then(|addr| self.nodes.get(addr)),
            ) {
                let load_diff = (current_info.load - min_load_info.load).abs();
                println!(
                    "DEBUG: Load difference between current leader and minimum load node: {}",
                    load_diff
                );
                if load_diff <= LOAD_THRESHOLD {
                    println!("DEBUG: Maintaining current leader due to load threshold");
                    return Some(current_leader.clone());
                }
            }
        }

        // Update current leader
        self.current_leader = min_load_node.clone();
        println!("DEBUG: New leader elected: {:?}", self.current_leader);
        min_load_node
    }

    fn get_node_info(&self, address: &str) -> Option<&NodeInfo> {
        self.nodes.get(address)
    }
}

impl LeaderProviderService {
    pub fn new(self_address: String, known_peers: Vec<String>) -> Self {
        let election = LeaderElection::new(self_address, known_peers);
        let state = LeaderState {
            election: Arc::new(Mutex::new(election)),
        };

        // Start periodic load broadcast
        let state_clone = state.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5)); // Broadcast every 5 seconds

            loop {
                interval.tick().await;
                let mut election = state_clone.election.lock().await;

                println!("DEBUG: Periodic load broadcast triggered");
                let current_load = election.get_current_load();
                if let Err(e) = election.broadcast_load(current_load).await {
                    println!("DEBUG: Failed to broadcast load periodically: {}", e);
                }
            }
        });

        LeaderProviderService { state }
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
        println!("\nDEBUG: Got a request for a leader provider!");

        let mut election = self.state.election.lock().await;

        let current_load = election.get_current_load();
        let self_address = election.self_address.clone();
        let self_rank = election.self_rank;

        println!(
            "DEBUG: Current node {} status - load: {}, rank: {}",
            self_address, current_load, self_rank
        );

        election.update_node(self_address.clone(), current_load, self_rank);

        if let Err(e) = election.broadcast_load(current_load).await {
            println!("DEBUG: Failed to broadcast load: {}", e);
        }

        let leader_address = match election.elect_leader() {
            Some(leader) => leader,
            None => {
                println!("DEBUG: No leader could be elected!");
                return Err(Status::internal("No available leader"));
            }
        };

        if let Some(leader_info) = election.get_node_info(&leader_address) {
            println!(
                "DEBUG: Election result - Leader: {} (load: {:.2}%, rank: {})",
                leader_address, leader_info.load, leader_info.rank
            );
        }

        let reply = leader_provider::LeaderProviderResponse {
            leader_socket: leader_address,
        };

        println!("DEBUG: Returning leader response: {}", reply.leader_socket);
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
    let port = std::env::var("PORT").unwrap_or_else(|_| "50051".to_string());
    let addr = format!("{}:{}", ip.to_string(), port).parse()?;
    let self_address = format!("{}:{}", ip.to_string(), port);

    println!("DEBUG: Starting server on {}", addr);

    // Configure known peers from environment or config file
    let known_peers = std::env::var("KNOWN_PEERS")
        .map(|peers| peers.split(',').map(String::from).collect())
        .unwrap_or_else(|_| {
            vec![
                "10.7.17.128:50051".to_string(),
                "10.7.16.11:50051".to_string(),
                "10.7.16.54:50051".to_string(),
            ]
        });

    println!("DEBUG: Known peers: {:?}", known_peers);

    let image_encoder_service = ImageEncoderService {};
    let leader_provider_service = LeaderProviderService::new(self_address.clone(), known_peers);
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
