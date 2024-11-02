use image::{imageops, DynamicImage::ImageRgba8, GenericImageView};
use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
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
) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, Box<dyn StdError + Send>> {
    print!("Started Encoding!");

    let dynamic_image =
        image::load_from_memory(&img).expect("Failed to convert bytes to DynamicImage");

    let (orig_width, orig_height) = dynamic_image.dimensions();

    // Calculate new dimensions while maintaining aspect ratio

    let blurred_img = imageops::blur(&dynamic_image, 20.0);
    print!("Generated blured");

    // Prepare JSON data to embed
    let data = EmbeddedData {
        message: "Hello, steganography!".to_string(),
        timestamp: "2024-10-27T00:00:00Z".to_string(),
    };

    // Serialize JSON data to bytes
    let json_data = to_vec(&data).expect("Failed to serialize JSON data");

    // Combine JSON data and original image bytes
    let combined_data: Vec<u8> = [json_data.clone(), img].concat();
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
    let encoded_img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> = my_encoder.encode_alpha();
    print!("Done!");

    // Return the encoded image data if successful
    Ok(encoded_img)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send>> {
    let main_socket = UdpSocket::bind("127.0.0.1:8080")
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
    println!("Main server listening on 127.0.0.1:8080");

    let port_counter = Arc::new(Mutex::new(8081));

    loop {
        let mut buf = [0; 1024];
        let (len, addr) = main_socket
            .recv_from(&mut buf)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        if len > 0 && buf[0] == 1 {
            // Assume 1 is the "send request" signal
            let new_port = {
                let mut counter = port_counter.lock().unwrap();
                let port = *counter;
                *counter += 1;
                port
            };

            // Send the new port number to the client
            let new_port_bytes = (new_port as u16).to_be_bytes();
            main_socket
                .send_to(&new_port_bytes, addr)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            println!("Assigned new port {} to client {}", new_port, addr);

            // Spawn a new task to handle the image transfer on the new port
            let handler_port = format!("127.0.0.1:{}", new_port);
            let handler_socket = UdpSocket::bind(&handler_port)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            task::spawn(handle_image_transfer(handler_socket, addr));
        }
    }
}

// Handler function to manage the image transfer on a new port
async fn handle_image_transfer(
    socket: UdpSocket,
    client_addr: SocketAddr,
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

    // Save the received image to a temporary file
    let encoded_image_result = encode_image(image_data.clone());
    let mut encoded_bytes: Vec<u8> = Vec::new();

    match encoded_image_result {
        Ok(encoded_image) => {
            // Convert ImageBuffer to Vec<u8>
            encoded_image.save("temp_image.jpeg");
            encoded_bytes = encoded_image.clone().into_raw(); // If using clone, ensure you handle ownership correctly
        }
        Err(e) => {
            eprintln!("Error encoding image: {}", e);
        }
    }

    // Step 2: Send the image back to the client
    let mut file = File::open("temp_image.jpeg")
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
    let mut image_data = Vec::new();
    file.read_to_end(&mut image_data)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

    let max_packet_size = 1022;
    let mut packet_number: u16 = 0;

    for chunk in image_data.chunks(max_packet_size) {
        let mut packet = Vec::with_capacity(2 + chunk.len());
        packet.extend_from_slice(&packet_number.to_be_bytes());
        packet.extend_from_slice(chunk);

        loop {
            socket.send_to(&packet, client_addr).await;
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

        packet_number += 1;
    }

    // Send end-of-transmission signal
    let terminator = [255, 255];
    socket.send_to(&terminator, client_addr).await;
    println!("Image sent back to client.");

    Ok(())
}
