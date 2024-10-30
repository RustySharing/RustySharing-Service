use tonic::{transport::Server, Request, Response, Status};
use image_encoding::image_encoder_server::{ImageEncoder, ImageEncoderServer};
use image_encoding::{EncodedImageRequest, EncodedImageResponse};

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

        // Call the encode_image function with the provided image data
        let encoded_image = match encode_image(image_data) { // Adjusted to use the image_data directly
            Ok(encoded_data) => encoded_data,
            Err(e) => {
                eprintln!("Error encoding image: {}", e);
                return Err(Status::internal("Image encoding failed"));
            },
        };

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
    let addr = "[::1]:50051".parse()?;
    let image_encoder_service = ImageEncoderService {};

    Server::builder()
        .add_service(ImageEncoderServer::new(image_encoder_service))
        .serve(addr)
        .await?;

    Ok(())
}
