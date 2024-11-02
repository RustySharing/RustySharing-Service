use image::{DynamicImage, ImageBuffer, Rgba};
use rpc_service::image_encoder::encode_image;
use rudp::{Endpoint, Guarantee, Sender};
use serde::{Deserialize, Serialize};
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::{error::Error, io::Cursor};

#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

async fn handle_request(
    data: Vec<u8>,
    endpoint: Arc<Mutex<Endpoint<UdpSocket>>>,
) -> Result<(), Box<dyn Error>> {
    // Call the encode_image function with the provided image data
    let encoded_image: ImageBuffer<Rgba<u8>, Vec<u8>> = match encode_image(data.clone()) {
        Ok(encoded_data) => encoded_data,
        Err(e) => {
            eprintln!("Error encoding image: {}", e);
            return Ok(());
        }
    };

    // Save the encoded image to a file
    let output_file_path = "encoded_image.png"; // Specify your output file path
    encoded_image
        .save(output_file_path)
        .expect("Failed to save encoded image");

    println!("Encoded image saved to {}", output_file_path);

    // Prepare to send the encoded image back to the client
    let mut encoded_image_data: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(encoded_image)
        .write_to(
            &mut Cursor::new(&mut encoded_image_data),
            image::ImageOutputFormat::PNG,
        )
        .expect("Failed to write encoded image to buffer");

    // Send the encoded image back to the client using the main thread's endpoint
    let send_result = {
        let mut endpoint_guard = endpoint.lock().unwrap();
        endpoint_guard.send_payload(&encoded_image_data, Guarantee::Delivery)
    };

    if let Err(e) = send_result {
        eprintln!("Error sending encoded image back: {}", e);
    } else {
        println!("Sent encoded image back");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let sock = UdpSocket::bind("0.0.0.0:50051")?;
    let endpoint = Arc::new(Mutex::new(Endpoint::new(sock)));
    println!("Listening on 0.0.0.0:50051");

    loop {
        // Maintain endpoint state
        {
            let endpoint_guard = endpoint.lock().unwrap();
            endpoint_guard.maintain();
        }

        // Attempt to receive data
        let data_option = {
            let mut endpoint_guard = endpoint.lock().unwrap();
            endpoint_guard.recv()
        };

        if let Ok(Some(data)) = data_option {
            let data_clone = data.to_vec();
            let endpoint_clone = Arc::clone(&endpoint);

            // Spawn a new task to handle the request
            tokio::spawn(async move {
                if let Err(e) = handle_request(data_clone, endpoint_clone).await {
                    eprintln!("Error handling request: {}", e);
                }
            });
        }
    }
}
