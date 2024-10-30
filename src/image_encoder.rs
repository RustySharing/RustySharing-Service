use std::fs::File;
use std::io::{self, Read};
use image::{self, imageops};
use serde::{Serialize, Deserialize};
use std::error::Error;

// Define your embedded data structure
#[derive(Serialize, Deserialize)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

// Function to encode an image with embedded data
pub fn encode_image(img_path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    // Step 1: Load the image
    let img = image::open(img_path)?;
    let blurred_img = imageops::blur(&img, 20.0);

    // Prepare JSON data to embed
    let data = EmbeddedData {
        message: "Hello, steganography!".to_string(),
        timestamp: "2024-10-27T00:00:00Z".to_string(),
    };
    
    // Serialize JSON data to bytes
    let json_data = serde_json::to_vec(&data).expect("Failed to serialize JSON data");

    // Read original image bytes
    let mut original_img_bytes = Vec::new();
    let mut original_file = File::open(img_path)?;
    original_file.read_to_end(&mut original_img_bytes)?;

    // Combine JSON data and original image bytes
    let combined_data: Vec<u8> = [json_data.clone(), original_img_bytes].concat();

    // Step 3: Encode JSON data into the image
    let my_encoder = encoder::Encoder::new(&combined_data, image::ImageRgba8(blurred_img.clone()));
    let encoded_img = my_encoder.encode_alpha();

    // Return the encoded image data
    Ok(encoded_img)
}