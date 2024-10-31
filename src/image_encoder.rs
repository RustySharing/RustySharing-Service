use image::{ self, imageops, DynamicImage, GenericImageView };

use serde::{ Serialize, Deserialize };
use steganography::encoder;
use std::error::Error;

// Define your embedded data structure
#[derive(Serialize, Deserialize)]
struct EmbeddedData {
  message: String,
  timestamp: String,
}

// Function to encode an image with embedded data
pub fn encode_image(img: DynamicImage) -> Result<Vec<u8>, Box<dyn Error>> {
  let blurred_img = imageops::blur(&img, 20.0);
  let (width, height) = blurred_img.dimensions();
  let low_res_img = imageops::resize(&blurred_img, width*13, height *13, image::imageops::FilterType::Lanczos3);

  // Prepare JSON data to embed
  let data = EmbeddedData {
    message: "Hello, steganography!".to_string(),
    timestamp: "2024-10-27T00:00:00Z".to_string(),
  };

  // Serialize JSON data to bytes
  let json_data = serde_json::to_vec(&data).expect("Failed to serialize JSON data");

  // img to bytes
  let img_in_bytes = img.to_rgba().into_vec();
  let img_in_bytes_len = img_in_bytes.len();

  // Combine JSON data and original image bytes
  let combined_data: Vec<u8> = [json_data.clone(), img_in_bytes].concat();

  // Check if combined data length exceeds image data capacity
  // if combined_data.len() > img_in_bytes_len {
  //   return Err("Combined data length exceeds image data capacity".into());
  // }

  // Step 3: Encode JSON data into the image
  let my_encoder = encoder::Encoder::new(&combined_data, image::ImageRgba8(low_res_img.clone()));
  let encoded_img = my_encoder.encode_image();

  // Return the encoded image data if successful
  Ok(encoded_img.to_vec())
}
