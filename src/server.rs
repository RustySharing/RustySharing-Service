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
use rpc_service::image_encoder::encode_image;

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
        ];

        LeaderProviderService {
            servers: Arc::new(RwLock::new(servers)),
            leader_index: Arc::new(Mutex::new(0)),
        }
    }

    // Perform health check for all servers
    async fn health_check(&self) {
        let mut servers = self.servers.write().await;

        for server in servers.iter_mut() {
            let now = Instant::now();
            if now.duration_since(server.last_checked) > Duration::from_secs(10) {
                server.is_healthy = self.is_server_healthy(&server.address).await;
                server.last_checked = now;
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
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    // Get the next leader using round-robin among healthy servers
    async fn get_next_leader(&self) -> Option<String> {
        let healthy_servers = self.get_healthy_servers().await;
        if healthy_servers.is_empty() {
            return None;
        }

        let mut leader_index = self.leader_index.lock().await;
        let leader = &healthy_servers[*leader_index % healthy_servers.len()];
        *leader_index = (*leader_index + 1) % healthy_servers.len();

        Some(leader.clone())
    }

    async fn get_healthy_servers(&self) -> Vec<String> {
        let servers = self.servers.read().await;
        servers.iter().filter(|s| s.is_healthy).map(|s| s.address.clone()).collect()
    }
}

// Implement Clone for LeaderProviderService
impl Clone for LeaderProviderService {
    fn clone(&self) -> Self {
        LeaderProviderService {
            servers: Arc::clone(&self.servers),
            leader_index: Arc::clone(&self.leader_index),
        }
    }
}

#[tonic::async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<leader_provider::LeaderProviderEmptyRequest>,
    ) -> Result<Response<leader_provider::LeaderProviderResponse>, Status> {
        self.health_check().await;

        match self.get_next_leader().await {
            Some(leader_socket) => {
                let reply = leader_provider::LeaderProviderResponse { leader_socket };
                Ok(Response::new(reply))
            }
            None => Err(Status::unavailable("No healthy servers available")),
        }
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

        // Call the encode_image function with the loaded image
        let encoded_image = match encode_image(image_data.clone(), image_name) {
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
            image_data: encoded_bytes.clone(),
        };

        // Delete file when done
        std::fs::remove_file(encoded_image)?;

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;

    let leader_provider_service = LeaderProviderService::new();

    // Spawn a background task to perform health checks periodically
    tokio::spawn({
        let leader_provider_service = leader_provider_service.clone();
        async move {
            loop {
                leader_provider_service.health_check().await;
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    });

    // Start the gRPC server
    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .add_service(ImageEncoderServer::new(ImageEncoderService {}))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .serve(addr)
        .await?;

    Ok(())
}
