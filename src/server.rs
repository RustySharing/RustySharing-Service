use tonic::{transport::Server, Request, Response, Status};
use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};

use local_ip;

// Import your encode_image function
use rpc_service::image_encoder::encode_image;

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
        let img = image::load_from_memory(&image_data)
            .map_err(|_| Status::internal("Failed to load image"))?;

        //Call the encode_image function with the provided image data
        let encoded_image = match encode_image(img) { // Dereference the reference to the byte slice
            Ok(encoded_data) => encoded_data,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            },
        };

        let encoded_photo = image::load_from_memory(&image_data)
            .map_err(|_| Status::internal("Failed to load image"))?;

        // Step 2: Save the image to a file
        let output_path = "output_image.jpeg"; // Specify your output file name
        encoded_photo.save(output_path)
            .map_err(|_| Status::internal("Failed to save image"))?;

        // Construct the response with the encoded image data
        let reply = EncodedImageResponse {
            width: request.width,
            height: request.height,
            image_data: encoded_image,
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
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .serve(addr)
        .await?;

    Ok(())
}
