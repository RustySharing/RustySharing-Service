use image_encoding::image_encoder_server::{ ImageEncoder, ImageEncoderServer };
use image_encoding::{ EncodedImageRequest, EncodedImageResponse };
use serde::{ Deserialize, Serialize };
use std::fs::File;
use std::str;
use steganography::util::file_to_bytes;
use tonic::{ transport::Server, Request, Response, Status };
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
    request: Request<EncodedImageRequest>
  ) -> Result<Response<EncodedImageResponse>, Status> {
    let request = request.into_inner();
    println!("Got a request!");

    // Get the image data from the request
    let image_data = &request.image_data; // Assuming image_data is passed as bytes

    // Step 1: Load the image from the byte data
    // let img = image::load_from_memory(image_data)
    //     .map_err(|_| Status::internal("Failed to load image from memory"))?;

    // Call the encode_image function with the loaded image
    let encoded_image = match encode_image(image_data.clone()) {
      // Pass the loaded image directly
      Ok(encoded_img_path) => encoded_img_path,
      Err(e) => {
        eprintln!("Error encoding image: {}", e);
        return Err(Status::internal("Image encoding failed"));
      }
    };

    let file = File::open(encoded_image)?;
    let encoded_bytes = file_to_bytes(file);
    // Construct the response with the encoded image data
    let reply = EncodedImageResponse {
      image_data: encoded_bytes.clone(), // Echo the original image data in the response
    };

    Ok(Response::new(reply))
  }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let ip = local_ip::get().unwrap();
  let addr = format!("{}:50051", ip.to_string()).parse()?;
  //let addr = "[::1]:50051".parse()?;
  let image_encoder_service = ImageEncoderService {};

  Server::builder()
    .max_frame_size(Some(10 * 1024 * 1024)) // Set to 10 MB
    .add_service(ImageEncoderServer::new(image_encoder_service))
    .serve(addr).await?;

  Ok(())
}
