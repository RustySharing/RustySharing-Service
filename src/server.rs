use clap::Parser;
use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use rand::Rng;
use rpc_service::image_encoder::encode_image;
use std::collections::HashMap;
use std::fs::File;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use steganography::util::file_to_bytes;
use sysinfo::System;
use tokio::sync::Mutex;
use tokio::time::interval;
use tokio::time::Duration;
use tonic::{transport::Server, Request, Response, Status};

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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Debug level: 0 = no debug, 1 = current node state only, 2 = all debug messages
    #[arg(short, long, default_value_t = 2)]
    debug_level: u8,
}

macro_rules! debug_print {
    ($debug_level:expr, $required_level:expr, $($arg:tt)*) => {
        if $debug_level >= $required_level {
            println!($($arg)*);
        }
    };
}

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
    debug_level: u8,
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
        let election_debug = self.state.election.lock().await.debug_level;

        debug_print!(
            election_debug,
            2,
            "DEBUG: Received load update from {}: load={}, rank={}",
            update.node_address,
            update.load,
            update.rank
        );

        let mut election = self.state.election.lock().await;
        let election_ref = &mut *election;
        election_ref.update_node(update.node_address, update.load, update.rank);

        Ok(Response::new(LoadUpdateResponse {}))
    }
}

impl LeaderElection {
    fn new(self_address: String, known_peers: Vec<String>, debug_level: u8) -> Self {
        let self_rank = rand::thread_rng().gen_range(0..u64::MAX);
        debug_print!(
            debug_level,
            1,
            "DEBUG: Initializing node {} with rank {}",
            self_address,
            self_rank
        );
        debug_print!(debug_level, 2, "DEBUG: Known peers: {:?}", known_peers);

        LeaderElection {
            nodes: HashMap::new(),
            current_leader: None,
            self_address,
            known_peers,
            system: System::new_all(),
            self_rank,
            debug_level,
        }
    }

    fn update_node(&mut self, address: String, load: f32, rank: u64) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        debug_print!(
            self.debug_level,
            2,
            "DEBUG: Updating node {} with load {} and rank {}",
            address,
            load,
            rank
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

        debug_print!(
            self.debug_level,
            1,
            "DEBUG: Current nodes state after update:"
        );
        for (addr, info) in &self.nodes {
            debug_print!(
                self.debug_level,
                1,
                "DEBUG: Node {}: load={}, rank={}, last_updated={}",
                addr,
                info.load,
                info.rank,
                info.last_updated
            );
        }
    }

    async fn broadcast_load(
        &mut self,
        load: f32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug_print!(
            self.debug_level,
            2,
            "DEBUG: Broadcasting load {} from node {}",
            load,
            self.self_address
        );

        let mut failed_nodes = Vec::new();
        let mut broadcast_errors = Vec::new();

        for peer in &self.known_peers.clone() {
            if peer != &self.self_address {
                debug_print!(
                    self.debug_level,
                    2,
                    "DEBUG: Attempting to send load update to peer {}",
                    peer
                );

                let timeout_duration = Duration::from_secs(2);
                let self_addr = self.self_address.clone();
                let self_rank = self.self_rank;

                match tokio::time::timeout(timeout_duration, async move {
                    match NodeCommunicatorClient::connect(format!("http://{}", peer)).await {
                        Ok(mut client) => {
                            let request = Request::new(LoadUpdate {
                                node_address: self_addr,
                                load,
                                rank: self_rank,
                            });

                            client.update_load(request).await
                        }
                        Err(e) => Err(Status::internal(format!("Failed to connect: {}", e))),
                    }
                })
                .await
                {
                    Ok(Ok(_)) => {
                        debug_print!(
                            self.debug_level,
                            2,
                            "DEBUG: Successfully sent load update to {}",
                            peer
                        );
                    }
                    Ok(Err(e)) => {
                        debug_print!(
                            self.debug_level,
                            2,
                            "DEBUG: Failed to send load update to {}: {}",
                            peer,
                            e
                        );
                        failed_nodes.push(peer.clone());
                        broadcast_errors.push(format!("Failed to send to {}: {}", peer, e));
                    }
                    Err(_) => {
                        debug_print!(
                            self.debug_level,
                            2,
                            "DEBUG: Timeout while sending load update to {}",
                            peer
                        );
                        failed_nodes.push(peer.clone());
                        broadcast_errors.push(format!("Timeout while sending to {}", peer));
                    }
                }
            }
        }

        // Remove failed nodes and print current state
        for failed_node in &failed_nodes {
            if self.nodes.remove(failed_node).is_some() {
                debug_print!(
                    self.debug_level,
                    2,
                    "DEBUG: Removed failed node {} from nodes list",
                    failed_node
                );
            }
        }

        debug_print!(self.debug_level, 1, "DEBUG: Nodes state after broadcast:");
        for (addr, info) in &self.nodes {
            debug_print!(
                self.debug_level,
                1,
                "DEBUG: Node {}: load={}, rank={}, last_updated={}",
                addr,
                info.load,
                info.rank,
                info.last_updated
            );
        }

        if !broadcast_errors.is_empty() {
            return Err(broadcast_errors.join(", ").into());
        }

        Ok(())
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

    fn update_self_load(&mut self) -> f32 {
        let current_load = self.get_current_load();
        self.update_node(self.self_address.clone(), current_load, self.self_rank);
        current_load
    }

    fn elect_leader(&mut self) -> Option<String> {
        // Remove stale nodes
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.nodes.retain(|_, info| {
            let is_fresh = current_time - info.last_updated < 10;
            if !is_fresh {
                debug_print!(
                    self.debug_level,
                    2,
                    "DEBUG: Node {} is stale, removing",
                    info.address
                );
            }
            is_fresh
        });

        // Find node with minimum load
        self.nodes
            .values()
            .min_by(|a, b| {
                let comp = a.load.partial_cmp(&b.load).unwrap();
                match comp {
                    std::cmp::Ordering::Equal => b.rank.cmp(&a.rank),
                    other => other,
                }
            })
            .map(|node| node.address.clone())
    }
}

impl LeaderProviderService {
    pub fn new(self_address: String, known_peers: Vec<String>, debug_level: u8) -> Self {
        let election = LeaderElection::new(self_address, known_peers, debug_level);
        let state = LeaderState {
            election: Arc::new(Mutex::new(election)),
        };

        // Start periodic self-update and broadcast task
        let state_clone = state.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1)); // Update every second

            loop {
                interval.tick().await;
                let mut election = state_clone.election.lock().await;

                // Update own load
                let current_load = election.update_self_load();
                debug_print!(
                    election.debug_level,
                    2,
                    "DEBUG: Updated self node with current load {}",
                    current_load
                );

                // Every 5 seconds, broadcast to peers
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                if timestamp % 5 == 0 {
                    debug_print!(
                        election.debug_level,
                        2,
                        "DEBUG: Periodic load broadcast triggered"
                    );
                    if let Err(e) = election.broadcast_load(current_load).await {
                        eprintln!("ERROR: Failed to broadcast load periodically: {}", e);
                    }
                }

                drop(election);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        // Start monitoring task
        let state_monitor = state.clone();
        tokio::spawn(async move {
            let mut monitor_interval = interval(Duration::from_secs(10));

            loop {
                monitor_interval.tick().await;
                let election = state_monitor.election.lock().await;
                debug_print!(
                    election.debug_level,
                    1,
                    "DEBUG: Monitor - Current nodes state:"
                );
                for (addr, info) in &election.nodes {
                    debug_print!(
                        election.debug_level,
                        1,
                        "DEBUG: Node {}: load={}, rank={}, last_updated={}",
                        addr,
                        info.load,
                        info.rank,
                        info.last_updated
                    );
                }
                drop(election);
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

        // Update own load before making decision
        let current_load = election.update_self_load();
        println!(
            "DEBUG: Current node {} status - load: {}, rank: {}",
            election.self_address, current_load, election.self_rank
        );

        // Broadcast current load to peers
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

        if let Some(leader_info) = election.nodes.get(&leader_address) {
            println!(
                "DEBUG: Election result - Leader: {} (load: {:.2}%, rank: {})",
                leader_address, leader_info.load, leader_info.rank
            );
        }

        let reply = leader_provider::LeaderProviderResponse {
            leader_socket: leader_address.clone(),
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
    let args = Args::parse();
    let debug_level = args.debug_level;

    let ip = local_ip::get().unwrap();
    let port = std::env::var("PORT").unwrap_or_else(|_| "50051".to_string());
    let addr = format!("{}:{}", ip, port).parse()?;
    let self_address = format!("{}:{}", ip, port);

    debug_print!(debug_level, 1, "DEBUG: Starting server on {}", addr);

    let known_peers = std::env::var("KNOWN_PEERS")
        .map(|peers| peers.split(',').map(String::from).collect())
        .unwrap_or_else(|_| {
            vec![
                "10.7.17.128:50051".to_string(),
                "10.7.16.11:50051".to_string(),
                "10.7.16.54:50051".to_string(),
            ]
        });

    debug_print!(debug_level, 2, "DEBUG: Known peers: {:?}", known_peers);

    let image_encoder_service = ImageEncoderService {};
    let leader_provider_service =
        LeaderProviderService::new(self_address.clone(), known_peers, debug_level);
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
