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
use tokio::time::{sleep, Duration};
use tonic::{transport::Server, Request, Response, Status};
// Import your encode_image function
use rpc_service::image_encoder::encode_image;

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
    system: System,
}

impl LeaderElection {
    fn new(self_address: String) -> Self {
        LeaderElection {
            nodes: HashMap::new(),
            current_leader: None,
            self_address,
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

        // Combine CPU and memory load with weights
        0.7 * cpu_load + 0.3 * memory_load
    }

    async fn update_node(&mut self, address: String, load: f32) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let rank = (load * 1000.0) as u64 + timestamp % 1000; // Create unique rank based on load and timestamp

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

    async fn elect_leader(&mut self) -> Option<String> {
        // Remove stale nodes (not updated in last 10 seconds)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.nodes
            .retain(|_, info| current_time - info.last_updated < 10);

        // Find node with minimum load
        self.nodes
            .values()
            .min_by(|a, b| a.load.partial_cmp(&b.load).unwrap())
            .map(|node| node.address.clone())
    }
}

struct LeaderState {
    election: Arc<Mutex<LeaderElection>>,
}

impl LeaderProviderService {
    pub fn new(self_address: String) -> Self {
        let election = LeaderElection::new(self_address);
        LeaderProviderService {
            state: LeaderState {
                election: Arc::new(Mutex::new(election)),
            },
        }
    }
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
struct LeaderProviderService {
    state: LeaderState,
}

#[tonic::async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<leader_provider::LeaderProviderEmptyRequest>,
    ) -> Result<Response<leader_provider::LeaderProviderResponse>, Status> {
        println!("Got a request for a leader provider!");

        let mut election = self.state.election.lock().await;

        // Update own load
        let current_load = election.get_current_load();
        election
            .update_node(election.self_address.clone(), current_load)
            .await;

        // Trigger election
        let leader_address = match election.elect_leader().await {
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
    let addr = format!("{}:50051", ip.to_string()).parse()?;
    let self_address = format!("{}:50051", ip.to_string());

    let image_encoder_service = ImageEncoderService {};
    let leader_provider_service = LeaderProviderService::new(self_address);

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service))
        .serve(addr)
        .await?;

    Ok(())
}
