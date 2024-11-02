use image::{GenericImageView, imageops};
use serde::{Serialize, Deserialize};
use serde_json::to_vec;
use steganography::encoder;
use std::error::Error;
use std::str;


#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}
fn calculate_image_dimensions_from_data(data_size: usize, bits_per_pixel: u32, aspect_ratio: (u32, u32)) -> (u32, u32) {
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



// Function to encode an image with embedded data
pub fn encode_image(img: Vec<u8>) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, Box<dyn Error>> {

  print!("Started Encoding!");
  
  let dynamic_image = image::load_from_memory(&img)
        .expect("Failed to convert bytes to DynamicImage");

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

  // Check if combined data length exceeds image data capacity
  // let img_len = img.len();
  // if combined_data.len() > img_len {
  //   return Err("Combined data length exceeds image data capacity".into());
  // }

  // Combine JSON data and original image bytes
  let combined_data: Vec<u8> = [json_data.clone(), img].concat();
  let aspect_ratio = calculate_aspect_ratio(orig_width, orig_height);
  let (new_width, new_height) = calculate_image_dimensions_from_data(combined_data.len(), 1, aspect_ratio);
  let resized_blurred_img = imageops::resize(&blurred_img, new_width, new_height, image::imageops::FilterType::Lanczos3);

  print!("Encoding ...");
  // Step 3: Encode JSON data into the image
  let my_encoder = encoder::Encoder::new(&combined_data, image::ImageRgba8(resized_blurred_img.clone()));
  let encoded_img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> = my_encoder.encode_alpha();
  print!("Done!");

  // Return the encoded image data if successful
  Ok(encoded_img)
}
