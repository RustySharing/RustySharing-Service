use tokio::net::UdpSocket;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::task;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let main_socket = UdpSocket::bind("127.0.0.1:8080").await?;
    println!("Main server listening on 127.0.0.1:8080");

    let port_counter = Arc::new(Mutex::new(8081));

    loop {
        let mut buf = [0; 1024];
        let (len, addr) = main_socket.recv_from(&mut buf).await?;

        if len > 0 && buf[0] == 1 { // Assume 1 is the "send request" signal
            let new_port = {
                let mut counter = port_counter.lock().unwrap();
                let port = *counter;
                *counter += 1;
                port
            };

            // Send the new port number to the client
            let new_port_bytes = (new_port as u16).to_be_bytes();
            main_socket.send_to(&new_port_bytes, addr).await?;
            println!("Assigned new port {} to client {}", new_port, addr);

            // Spawn a new task to handle the image transfer on the new port
            let handler_port = format!("127.0.0.1:{}", new_port);
            let handler_socket = UdpSocket::bind(&handler_port).await?;
            task::spawn(handle_image_transfer(handler_socket, addr));
        }
    }
}

// Handler function to manage the image transfer on a new port
async fn handle_image_transfer(socket: UdpSocket, client_addr: SocketAddr) -> std::io::Result<()> {
    println!("Started image transfer handler on {}", socket.local_addr()?);

    let mut buf = [0; 1024 + 2]; // Buffer for receiving data (packet size + 2 bytes for sequence number)
    let mut received_packets: HashMap<u16, Vec<u8>> = HashMap::new();
    let mut total_packets = 0;

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;

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

        socket.send_to(&packet_number.to_be_bytes(), addr).await?;
        println!("Received packet {} and sent acknowledgment.", packet_number);
    }

    let mut image_data = Vec::new();
    for i in 0..total_packets {
        if let Some(chunk) = received_packets.remove(&i) {
            image_data.extend(chunk);
        }
    }

    let mut file = File::create("received_image.jpeg")?;
    file.write_all(&image_data)?;
    println!("Image saved as 'received_image.jpeg'.");

    Ok(())
}