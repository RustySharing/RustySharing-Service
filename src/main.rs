use image::{imageops, DynamicImage::ImageRgba8, GenericImageView};
use image::{ImageBuffer, Rgba};
use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::Ipv4Addr;
use std::str;
use std::sync::{Arc, Mutex};
use steganography::encoder;
use tokio::net::UdpSocket;
use tokio::task;

#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

fn calculate_image_dimensions_from_data(
    data_size: usize,
    bits_per_pixel: u32,
    aspect_ratio: (u32, u32),
) -> (u32, u32) {
    // Calculate the total number of bits needed
    let total_bits_needed = data_size * 8; // Convert bytes to bits

    // Calculate the number of pixels needed
    let pixels_needed = (total_bits_needed + bits_per_pixel as usize - 1) / bits_per_pixel as usize; // Round up

    // Aspect ratio
    let (aspect_width, aspect_height) = aspect_ratio;

    // Calculate the total aspect ratio
    let total_aspect = aspect_width as f32 / aspect_height as f32;

    // Calculate width and height based on the aspect ratio
    let height = (pixels_needed as f32 / total_aspect.sqrt()).sqrt().round() as u32;
    let width = (height as f32 * total_aspect).round() as u32;

    (width, height)
}

fn calculate_aspect_ratio(width: u32, height: u32) -> (u32, u32) {
    // Calculate the greatest common divisor (GCD) using the Euclidean algorithm
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }

    let divisor = gcd(width, height);
    (width / divisor, height / divisor)
}

use std::error::Error as StdError;

pub fn encode_image(
    img: Vec<u8>,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, Box<dyn StdError + Send>> {
    println!("Started Encoding!");

    let dynamic_image =
        image::load_from_memory(&img).expect("Failed to convert bytes to DynamicImage");

    let (orig_width, orig_height) = dynamic_image.dimensions();

    // Apply a blur to the image
    let blurred_img = imageops::blur(&dynamic_image, 20.0);
    println!("Generated blurred image");

    // Prepare JSON data to embed
    let data = EmbeddedData {
        message: "Hello, steganography!".to_string(),
        timestamp: "2024-10-27T00:00:00Z".to_string(),
    };

    // Serialize JSON data to bytes
    let json_data = to_vec(&data).expect("Failed to serialize JSON data");
    println!("Serialized JSON: {}", String::from_utf8_lossy(&json_data));

    let img_len = img.len();

    // Combine JSON data and original image bytes, ensuring the JSON data is at the beginning
    let combined_data: Vec<u8> = [json_data.clone(), img].concat();

    println!("JSON length: {}", json_data.len());
    println!("Image length: {}", img_len);
    println!("Combined data length: {}", combined_data.len());

    let aspect_ratio = calculate_aspect_ratio(orig_width, orig_height);
    let (new_width, new_height) =
        calculate_image_dimensions_from_data(combined_data.len(), 1, aspect_ratio);

    let resized_blurred_img = imageops::resize(
        &blurred_img,
        new_width,
        new_height,
        image::imageops::FilterType::Lanczos3,
    );

    print!("Encoding ...");
    // Step 3: Encode JSON data into the image
    let my_encoder = encoder::Encoder::new(&combined_data, ImageRgba8(resized_blurred_img.clone()));
    let encoded_img: ImageBuffer<Rgba<u8>, Vec<u8>> = my_encoder.encode_alpha();
    println!("Done!");

    // Return the encoded image data if successful
    Ok(encoded_img)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send>> {
    let server_list = ["10.7.16.54".to_string(), "10.7.17.128".to_string()];

    let multicast_addr: Ipv4Addr = "239.255.0.1".parse().unwrap();
    let multicast_port = 9001;
    let my_addr = "10.7.16.54"; // Replace with this server's IP address

    let local_socket = "0.0.0.0:9001"; // Bind to all interfaces on port 9001

    // Create and bind the UDP socket
    let socket = UdpSocket::bind(local_socket)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
    println!("Server bound to {}", local_socket);

    // Join the multicast group on the specified local interface
    socket
        .join_multicast_v4(multicast_addr, my_addr.parse::<Ipv4Addr>().unwrap())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
    println!(
        "Server joined multicast group {} on interface {}",
        multicast_addr, my_addr
    );

    // Initialize the talking stick status (true if this server should start with the talking stick)
    let my_index = server_list.iter().position(|x| x == my_addr).unwrap();
    let next_server_addr = &server_list[(my_index + 1) % server_list.len()];
    let has_token = Arc::new(Mutex::new(my_index == 0));

    loop {
        let mut buf = [0; 1024];
        println!("Waiting to receive data...");
        let (len, client_addr) = socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        // Log that data was received
        println!("Received packet of length {} from {}", len, client_addr);

        // Print talking stick status for debugging
        {
            let has_token = has_token.lock().unwrap();
            if *has_token {
                println!(
                    "Server {} has the talking stick and received a request from {}",
                    my_addr, client_addr
                );
            } else {
                println!(
                    "Server {} does NOT have the talking stick but received a request from {}",
                    my_addr, client_addr
                );
            }
        }

        if len == 1 && buf[0] == 99 {
            // Token received from the previous server
            let mut token = has_token.lock().unwrap();
            *token = true;
            println!("Server {} received the talking stick", my_addr);
            continue;
        }

        if *has_token.lock().unwrap() {
            // Server has the talking stick and should proceed to handle the request
            if len > 0 && buf[0] == 1 {
                let mut token = has_token.lock().unwrap();
                *token = false;
                socket
                    .send_to(&[99], format!("{}:{}", next_server_addr, multicast_port))
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
                println!(
                    "Server {} passed the talking stick to {}",
                    my_addr, next_server_addr
                );
                // Generate a new port for this client and create a handler
                let handler_port = 10000 + rand::random::<u16>() % 1000;
                let handler_socket =
                    UdpSocket::bind((my_addr.parse::<Ipv4Addr>().unwrap(), handler_port))
                        .await
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

                // Construct the 6-byte response (4 bytes for IP, 2 bytes for port)
                let ip_bytes = my_addr.parse::<Ipv4Addr>().unwrap().octets();
                let port_bytes = (handler_port as u16).to_be_bytes();
                let mut response = Vec::new();
                response.extend_from_slice(&ip_bytes);
                response.extend_from_slice(&port_bytes);

                // Send the 6-byte response to the client
                println!("Sending response to client: {:?}", response);
                socket
                    .send_to(&response, client_addr)
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

                println!("Assigned port {} to client {}", handler_port, client_addr);

                // Spawn a new task to handle the image transfer on the new port
                let client_addr_clone = client_addr;
                task::spawn(async move {
                    let _ = handle_image_transfer(handler_socket, client_addr_clone).await;
                });

                // Release the talking stick and pass it to the next server

            }
        } else {
            // Ignore the request if we don't have the talking stick
            println!(
                "Server {} received a request but does not have the talking stick; ignoring.",
                my_addr
            );
        }
    }
}

// Handler function to manage the image transfer on a new port
async fn handle_image_transfer(
    socket: UdpSocket,
    client_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn StdError + Send>> {
    match socket.local_addr() {
        Ok(addr) => println!("Started image transfer handler on {}", addr),
        Err(e) => eprintln!("Failed to get local address: {}", e),
    }

    let mut buf = [0; 1024 + 2];
    let mut received_packets: HashMap<u16, Vec<u8>> = HashMap::new();
    let mut total_packets = 0;

    // Step 1: Receive the image from the client
    loop {
        let (len, addr) = socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        if addr != client_addr {
            println!("Ignoring packet from unknown client {}", addr);
            continue;
        }

        if len == 2 && buf[0] == 255 && buf[1] == 255 {
            println!("End of transmission signal received.");
            break;
        }

        let packet_number = u16::from_be_bytes([buf[0], buf[1]]);
        let data = buf[2..len].to_vec();
        received_packets.insert(packet_number, data);
        total_packets = total_packets.max(packet_number + 1);

        let _ = socket.send_to(&packet_number.to_be_bytes(), addr).await;
        println!("Received packet {} and sent acknowledgment.", packet_number);
    }

    let mut image_data = Vec::new();
    for i in 0..total_packets {
        if let Some(chunk) = received_packets.remove(&i) {
            image_data.extend(chunk);
        }
    }

    // if image_data.len() < 2 || image_data[0] != 0xFF || image_data[1] != 0xD8 {
    //     eprintln!("Received data is not a valid JPEG image.");
    // }

    //let img: DynamicImage = load_from_memory(&image_data).expect("Failed to load image from bytes");

    // Optionally, save the image back to a file to verify

    // Save the received image to a temporary file
    let encoded_image_result = encode_image(image_data.clone());
    // let mut encoded_bytes: Vec<u8> = Vec::new();
    let output_path = "temp_image.png";
    match encoded_image_result {
        Ok(encoded_image) => {
            // Convert ImageBuffer to Vec<u8>
            let _ = encoded_image.save(output_path);
            //encoded_bytes = encoded_image.clone().into_raw(); // If using clone, ensure you handle ownership correctly
        }
        Err(e) => {
            eprintln!("Error encoding image: {}", e);
        }
    }

    // Step 2: Send the image back to the client
    let mut file =
        File::open(output_path).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
    let mut image_data = Vec::new();
    file.read_to_end(&mut image_data)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

    let max_packet_size = 1024;
    //let mut packet_number: u16 = 0;

    for (packet_number, chunk) in (0_u16..).zip(image_data.chunks(max_packet_size)) {
        let mut packet = Vec::with_capacity(2 + chunk.len());
        packet.extend_from_slice(&packet_number.to_be_bytes());
        packet.extend_from_slice(chunk);

        loop {
            let _ = socket.send_to(&packet, client_addr).await;
            println!("Sent packet {}", packet_number);

            let mut ack_buf = [0; 2];
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(1),
                socket.recv_from(&mut ack_buf),
            )
            .await
            {
                Ok(Ok((_, _))) => {
                    let ack_packet_number = u16::from_be_bytes(ack_buf);
                    if ack_packet_number == packet_number {
                        println!("Acknowledgment received for packet {}", packet_number);
                        break;
                    }
                }
                _ => {
                    println!(
                        "No acknowledgment received for packet {}, resending...",
                        packet_number
                    );
                }
            }
        }

        //packet_number += 1;
    }

    let terminator = [255, 255];
    let _ = socket.send_to(&terminator, client_addr).await;
    println!("Image sent back to client.");

    Ok(())
}
