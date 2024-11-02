use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};
use serde::{Deserialize, Serialize};
use std::str;
use steganography::decoder;
use tonic::{transport::Server, Request, Response, Status};
// Import your encode_image function
use rpc_service::image_encoder::encode_image;

#[derive(Serialize, Deserialize, Debug)]
struct EmbeddedData {
    message: String,
    timestamp: String,
}

// This module is generated from your .proto file
pub mod image_encoding {
    tonic::include_proto!("image_encoding");
}

// Define your ImageEncoderService
struct ImageEncoderService {}

#[tonic::async_trait]
impl ImageEncoder for ImageEncoderService {
    async fn image_encode(
        &self,
        request: Request<EncodedImageRequest>,
    ) -> Result<Response<EncodedImageResponse>, Status> {
        let request = request.into_inner();
        println!("Got a request: {:?}", request);

        // Get the image data from the request
        let image_data = &request.image_data; // Assuming image_data is passed as bytes

        // Step 1: Load the image from the byte data
        // let img = image::load_from_memory(image_data)
        //     .map_err(|_| Status::internal("Failed to load image from memory"))?;

        // Call the encode_image function with the loaded image
        let encoded_image: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            match encode_image(image_data.clone()) {
                // Pass the loaded image directly
                Ok(encoded_data) => encoded_data,
                Err(e) => {
                    eprintln!("Error encoding image: {}", e);
                    return Err(Status::internal("Image encoding failed"));
                }
            };

        let (encoded_width, encoded_height) = encoded_image.dimensions();
        let encoded_bytes = encoded_image.clone().into_raw();
        // Save the encoded image to a file if needed (optional)
        let output_file_path = "encoded_image.png"; // Specify your output file path
        encoded_image
            .save(output_file_path)
            .expect("Failed to save encoded image");

        println!("Encoded image saved to {}", output_file_path);

        // Decode the encoded image
        let decoded_img = image::open(output_file_path).expect("Failed to open encoded image");
        let my_decoder = decoder::Decoder::new(decoded_img.to_rgba());
        let decoded_data = my_decoder.decode_alpha();

        // Find the position of the JSON content
        let start = decoded_data
            .iter()
            .position(|&b| b == b'{')
            .expect("Opening brace not found");
        let end = decoded_data
            .iter()
            .position(|&b| b == b'}')
            .expect("Closing brace not found");

        let json_part = &decoded_data[start..=end]; // Include the closing brace
        let original_image_part = &decoded_data[end + 1..]; // Skip past the closing brace

        let decoded_json: EmbeddedData =
            serde_json::from_slice(json_part).expect("Failed to parse JSON data");
        println!("Decoded Data: {:?}", decoded_json);

        // Save the extracted original image (optional)
        let original_image_output_path = "extracted_original_image.png";
        std::fs::write(original_image_output_path, original_image_part)
            .expect("Failed to save the extracted original image");
        println!(
            "Extracted original image saved as: {}",
            original_image_output_path
        );

        // Construct the response with the encoded image data
        let reply = EncodedImageResponse {
            width: encoded_width as i32,
            height: encoded_height as i32,
            image_data: encoded_bytes.clone(), // Echo the original image data in the response
        };

        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ip = local_ip::get().unwrap();
    let addr = format!("{}:50051", ip.to_string()).parse()?;
    let image_encoder_service = ImageEncoderService {};

    Server::builder()
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .serve(addr)
        .await?;

    Ok(())
}
