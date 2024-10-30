use tonic::{ transport::Server, Request, Response, Status };

use hello::hello_server::{ Hello, HelloServer };
use hello::{ HelloRequest, HelloResponse };

//TODO: actually have an image service through grpc
pub mod hello {
  tonic::include_proto!("hello");
}

struct HelloService {}

#[tonic::async_trait]
impl Hello for HelloService {
  async fn send_hello(
    &self,
    request: Request<HelloRequest>
  ) -> Result<Response<HelloResponse>, Status> {
    let request = request.into_inner();
    println!("Got a request: {:?}", request); 

    //image encode 
    //then send in reply

    let reply = hello::HelloResponse {
      message: format!("Acknowledged:: {}!", request.name),
    };

    Ok(Response::new(reply))
  }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let addr = "[::1]:50051".parse()?;
  let hello_service = HelloService {};

  Server::builder().add_service(HelloServer::new(hello_service)).serve(addr).await?;

  Ok(())
}
