use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use leader_provider::leader_provider_server::{LeaderProvider, LeaderProviderServer};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use steganography::util::file_to_bytes;
use tonic::{transport::Server, Request, Response, Status};
use rpc_service::image_encoder::encode_image;
use local_ip;

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

// Struct to hold server IP and load metrics
#[derive(Clone)]
struct ServerInfo {
    ip: String,
    load_metric: u64, // Example load metric (e.g., CPU usage)
}

#[derive(Clone)]
struct LeaderProviderService {
    servers: Vec<ServerInfo>,                 // List of server IPs
    leader: Arc<Mutex<Option<String>>>,        // Leader IP address
}

impl LeaderProviderService {
    // Function to send and receive load metrics using TCP on port 6000
    fn send_load_metric(&self, server_ip: &str, load_metric: u64) -> Option<u64> {
        println!("Attempting to connect to server at {}:6000", server_ip);

        match TcpStream::connect(format!("{}:6000", server_ip)) {
            Ok(mut stream) => {
                println!("Connected to server at {}:6000", server_ip);
                
                let msg = load_metric.to_string();
                if stream.write_all(msg.as_bytes()).is_err() {
                    println!("Failed to send load metric to {}", server_ip);
                    return None;
                }

                let mut buffer = [0; 128];
                match stream.read(&mut buffer) {
                    Ok(bytes_read) => {
                        let response = String::from_utf8_lossy(&buffer[..bytes_read]);
                        match response.trim().parse::<u64>() {
                            Ok(parsed_load) => {
                                println!("Received load metric from {}: {}", server_ip, parsed_load);
                                Some(parsed_load)
                            },
                            Err(_) => {
                                println!("Failed to parse load metric from server {}", server_ip);
                                None
                            }
                        }
                    }
                    Err(_) => {
                        println!("Failed to read response from server {}", server_ip);
                        None
                    }
                }
            }
            Err(e) => {
                println!("Failed to connect to server at {}:6000 - {}", server_ip, e);
                None
            }
        }
    }

    // Function to perform leader election by comparing load metrics
    fn elect_leader(&self, my_load_metric: u64) -> Option<String> {
        println!("Starting leader election...");

        let mut load_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let local_ip = local_ip::get().unwrap().to_string();
        load_map.insert(local_ip.clone(), my_load_metric);

        for server in &self.servers {
            if let Some(load) = self.send_load_metric(&server.ip, my_load_metric) {
                load_map.insert(server.ip.clone(), load);
            } else {
                println!("No load metric received from server at {}", server.ip);
            }
        }

        let (leader_ip, _) = load_map.iter().min_by_key(|&(_, &load)| load)?;
        println!("Elected leader: {}", leader_ip);

        *self.leader.lock().unwrap() = Some(leader_ip.clone());
        Some(leader_ip.clone())
    }

    // Start the listener to respond with the server’s load metric on port 6000
    fn start_listener(&self) {
        println!("Starting listener on port 6000...");
        let listener = TcpListener::bind("0.0.0.0:6000").expect("Failed to bind listener");

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    println!("Accepted connection from {:?}", stream.peer_addr());
                    let mut buffer = [0; 128];
                    
                    match stream.read(&mut buffer) {
                        Ok(bytes_read) => {
                            let received_load = String::from_utf8_lossy(&buffer[..bytes_read]);
                            let _load: u64 = received_load.trim().parse().expect("Failed to parse load");
                            println!("Received load request: {}", received_load);

                            let my_load_metric = 10; // Replace with actual load calculation
                            let response = my_load_metric.to_string();
                            if stream.write_all(response.as_bytes()).is_ok() {
                                println!("Sent load metric: {}", response);
                            } else {
                                println!("Failed to send load metric response");
                            }
                        }
                        Err(e) => {
                            println!("Failed to read from stream: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

#[tonic::async_trait]
impl LeaderProvider for LeaderProviderService {
    async fn get_leader(
        &self,
        _request: Request<leader_provider::LeaderProviderEmptyRequest>,
    ) -> Result<Response<leader_provider::LeaderProviderResponse>, Status> {
        println!("Starting leader election on a separate thread...");

        let leader = self.leader.clone();
        let servers = self.servers.clone();
        let my_load_metric = 15; // Replace with actual load calculation

        // Start leader election in a separate thread
        thread::spawn(move || {
            let elected_leader = LeaderProviderService {
                servers,
                leader,
            }.elect_leader(my_load_metric);

            if let Some(leader_ip) = elected_leader {
                println!("Leader elected: {}", leader_ip);
            } else {
                println!("Leader election failed.");
            }
        });

        // Return the current leader if already elected
        let current_leader = self.leader.lock().unwrap();
        if let Some(ref leader_ip) = *current_leader {
            let reply = leader_provider::LeaderProviderResponse {
                leader_socket: format!("{}:50051", leader_ip),
            };
            Ok(Response::new(reply))
        } else {
            Err(Status::unavailable("Leader election in progress"))
        }
    }
}

// Define your ImageEncoderService with a leader check
struct ImageEncoderService {
    leader_ip: Arc<Mutex<Option<String>>>, // IP address of the elected leader
}

#[tonic::async_trait]
impl ImageEncoder for ImageEncoderService {
    async fn image_encode(
        &self,
        request: Request<EncodedImageRequest>,
    ) -> Result<Response<EncodedImageResponse>, Status> {
        let request = request.into_inner();
        println!("Received an image encoding request");

        // Check if this server is the elected leader
        let current_leader = self.leader_ip.lock().unwrap();
        let local_ip = local_ip::get().unwrap().to_string();
        if current_leader.as_deref() != Some(&local_ip) {
            eprintln!("This server is not the leader; rejecting request.");
            return Err(Status::permission_denied("Only the leader can encode images"));
        }

        // Proceed with encoding since this server is the leader
        let image_data = &request.image_data;
        let image_name = &request.file_name;

        let encoded_image = match encode_image(image_data.clone(), image_name) {
            Ok(encoded_img_path) => encoded_img_path,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            }
        };

        let file = File::open(encoded_image.clone()).map_err(|_| Status::internal("Failed to open encoded file"))?;
        let encoded_bytes = file_to_bytes(file);
        
        let reply = EncodedImageResponse {
            image_data: encoded_bytes,
        };

        std::fs::remove_file(encoded_image).map_err(|_| Status::internal("Failed to delete temporary file"))?;
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;
    
    let leader_ip = Arc::new(Mutex::new(None));
    let leader_provider_service = LeaderProviderService {
        servers: vec![
            ServerInfo { ip: "10.17.7.128".to_string(), load_metric: 0 },
            ServerInfo { ip: "10.7.16.11".to_string(), load_metric: 0 },
        ],
        leader: leader_ip.clone(),
    };

    let image_encoder_service = ImageEncoderService {
        leader_ip: leader_ip.clone(),
    };

    let service_clone = leader_provider_service.clone();
    thread::spawn(move || {
        service_clone.start_listener();
    });

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024))
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .add_service(LeaderProviderServer::new(leader_provider_service.clone()))
        .serve(addr)
        .await?;

    Ok(())
}
