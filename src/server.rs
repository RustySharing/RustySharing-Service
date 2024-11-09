use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::str;
use steganography::util::file_to_bytes;
use tonic::{transport::Server, Request, Response, Status};
use tokio::sync::{RwLock, Mutex};
use std::sync::Arc;
use std::process::Command;
use std::time::{Duration, Instant};
use async_trait::async_trait;

#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

// These modules are generated from your .proto files
pub mod image_encoding {
    tonic::include_proto!("image_encoding");
}

pub mod leader_provider {
    tonic::include_proto!("leader_provider");
}

// Define your ImageEncoderService
struct ImageEncoderService {}

struct LeaderProviderService {
    servers: Arc<RwLock<Vec<ServerInfo>>>, // List of servers for leader election
    leader_index: Arc<Mutex<usize>>,       // Round-robin leader index
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    address: String,
    last_checked: Instant,
    is_healthy: bool,
}

impl LeaderProviderService {
    fn new() -> Self {
        let servers = vec![
            ServerInfo {
                address: "10.7.17.128:50051".to_string(),
                last_checked: Instant::now(),
                is_healthy: true,
            },
            ServerInfo {
                address: "10.7.16.11:50051".to_string(),
                last_checked: Instant::now(),
                is_healthy: true,
            },
            ServerInfo {
                address: "10.7.16.54:50051".to_string(),
                last_checked: Instant::now(),
                is_healthy: true,
            },
        ];

        LeaderProviderService {
            servers: Arc::new(RwLock::new(servers)),
            leader_index: Arc::new(Mutex::new(0)),
        }
    }

    // Perform health check for all servers
    async fn health_check(&self) {
        println!("Performing health check for all servers...");

        let mut servers = self.servers.write().await;

        for server in servers.iter_mut() {
            let now = Instant::now();
            if now.duration_since(server.last_checked) > Duration::from_secs(10) {
                println!("Checking server {}...", server.address);
                server.is_healthy = self.is_server_healthy(&server.address).await;
                server.last_checked = now;

                // Log the result of the health check for this server
                if server.is_healthy {
                    println!("Server {} is healthy.", server.address);
                } else {
                    println!("Server {} is unhealthy.", server.address);
                }
            }
        }
    }

    // Simulate a health check (could be ping, HTTP request, etc.)
    async fn is_server_healthy(&self, address: &str) -> bool {
        let ip = address.split(':').next().unwrap();
        let output = Command::new("ping")
            .arg("-c 1")
            .arg(ip)
            .output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    println!("Ping successful for {}.", ip);
                    true
                } else {
                    println!("Ping failed for {}. Marking as unhealthy.", ip);
                    false
                }
            },
            Err(_) => {
                println!("Failed to ping {}. Marking as unhealthy.", ip);
                false
            }
        }
    }

    // Get the next leader using round-robin among healthy servers
    async fn get_next_leader(&self) -> Option<String> {
        let healthy_servers = self.get_healthy_servers().await;
        
        if healthy_servers.is_empty() {
            println!("No healthy servers available for election.");
            return None;
        }

        // Round-robin election
        let mut leader_index = self.leader_index.lock().await;
        let leader = &healthy_servers[*leader_index % healthy_servers.len()];

        // Log the leader election process
        println!("Round-robin leader election: selected leader = {}", leader.address);

        // Move to the next leader for the next election
        *leader_index = (*leader_index + 1) % healthy_servers.len();

        Some(leader.address.clone())
    }

    // Get only healthy servers
    async fn get_healthy_servers(&self) -> Vec<ServerInfo> {
        let servers = self.servers.read().await;
        let healthy_servers: Vec<ServerInfo> = servers
            .iter()
            .filter(|s| s.is_healthy)
            .cloned()
            .collect();

        // Log the healthy servers
        if healthy_servers.is_empty() {
            println!("No healthy servers found.");
        } else {
            println!("Healthy servers found: ");
            for server in &healthy_servers {
                println!("- {}", server.address);
            }
        }

        healthy_servers
    }
}

// Define LeaderProvider methods
#[async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<leader_provider::LeaderProviderEmptyRequest>,
    ) -> Result<Response<leader_provider::LeaderProviderResponse>, Status> {
        println!("Received request for leader election...");

        // Perform health check on servers
        self.health_check().await;

        // Get the elected leader
        match self.get_next_leader().await {
            Some(leader_address) => {
                println!("Leader elected: {}", leader_address);
                let reply = leader_provider::LeaderProviderResponse {
                    leader_socket: leader_address,
                };
                Ok(Response::new(reply))
            }
            None => {
                println!("No leader could be elected.");
                Err(Status::unavailable("No available leader"))
            }
        }
    }
}

// Image encoding service implementation (unchanged)
#[tonic::async_trait]
impl ImageEncoder for ImageEncoderService {
    async fn image_encode(
        &self,
        request: Request<EncodedImageRequest>,
    ) -> Result<Response<EncodedImageResponse>, Status> {
        let request = request.into_inner();
        println!("Received request to encode image: {}", request.file_name);

        // (Image encoding logic remains unchanged)

        Ok(Response::new(EncodedImageResponse {
            image_data: vec![], // Placeholder for encoded image data
        }))
    }
}

// Start the server
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Remove env_logger initialization; no need for it if we're just using println!

    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;
    let image_encoder_service = ImageEncoderService {};
    let leader_provider_service = LeaderProviderService::new();

    println!("Starting server on {}", addr);

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .serve(addr)
        .await?;

    Ok(())
}
