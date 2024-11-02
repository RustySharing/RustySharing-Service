use tokio::net::UdpSocket;
use tokio::task;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write, Read};
use std::time::Duration;
use std::net::Ipv4Addr;

#[tokio::main]
async fn main() -> io::Result<()> {
    let server_list = vec![
        "10.7.16.54".to_string(),
        "10.7.17.128".to_string(),
    ];

    let multicast_addr: Ipv4Addr = "239.255.0.1".parse().unwrap();
    let multicast_port = 9001;
    let my_addr = "10.7.17.128"; // Replace with this server's IP address

    let local_socket = "0.0.0.0:9001"; // Bind to all interfaces on port 9001

    // Create and bind the UDP socket
    let socket = UdpSocket::bind(local_socket).await?;
    println!("Server bound to {}", local_socket);

    // Join the multicast group on the specified local interface
    socket.join_multicast_v4(multicast_addr, my_addr.parse::<Ipv4Addr>().unwrap())?;
    println!("Server joined multicast group {} on interface {}", multicast_addr, my_addr);

    // Initialize the talking stick status (true if this server should start with the talking stick)
    let my_index = server_list.iter().position(|x| x == my_addr).unwrap();
    let next_server_addr = &server_list[(my_index + 1) % server_list.len()];
    let has_token = Arc::new(Mutex::new(my_index == 0));

    loop {
        let mut buf = [0; 1024];
        println!("Waiting to receive data...");
        let (len, client_addr) = socket.recv_from(&mut buf).await?;

        // Log that data was received
        println!("Received packet of length {} from {}", len, client_addr);

        // Print talking stick status for debugging
        {
            let has_token = has_token.lock().unwrap();
            if *has_token {
                println!("Server {} has the talking stick and received a request from {}", my_addr, client_addr);
            } else {
                println!("Server {} does NOT have the talking stick but received a request from {}", my_addr, client_addr);
            }
        }

        if len == 1 && buf[0] == 99 {
            // Token received from the previous server
            let mut token = has_token.lock().unwrap();
            *token = true;
            println!("Server {} received the talking stick", my_addr);
            continue;
        }

        if *has_token.lock().unwrap() {
            // Server has the talking stick and should proceed to handle the request
            if len > 0 && buf[0] == 1 {
                // Generate a new port for this client and create a handler
                let handler_port = 10000 + rand::random::<u16>() % 1000;
                let handler_socket = UdpSocket::bind((my_addr.parse::<Ipv4Addr>().unwrap(), handler_port)).await?;

                // Construct the 6-byte response (4 bytes for IP, 2 bytes for port)
                let ip_bytes = my_addr.parse::<Ipv4Addr>().unwrap().octets();
                let port_bytes = (handler_port as u16).to_be_bytes();
                let mut response = Vec::new();
                response.extend_from_slice(&ip_bytes);
                response.extend_from_slice(&port_bytes);

                // Send the 6-byte response to the client
                println!("Sending response to client: {:?}", response);
                socket.send_to(&response, client_addr).await?;

                println!("Assigned port {} to client {}", handler_port, client_addr);

                // Spawn a new task to handle the image transfer on the new port
                let client_addr_clone = client_addr.clone();
                task::spawn(async move {
                    handle_image_transfer(handler_socket, client_addr_clone).await;
                });

                // Release the talking stick and pass it to the next server
                let mut token = has_token.lock().unwrap();
                *token = false;
                socket.send_to(&[99], format!("{}:{}", next_server_addr, multicast_port)).await?;
                println!("Server {} passed the talking stick to {}", my_addr, next_server_addr);
            }
        } else {
            // Ignore the request if we don't have the talking stick
            println!("Server {} received a request but does not have the talking stick; ignoring.", my_addr);
        }
    }
}

// Function to handle image transfer on a unique port
async fn handle_image_transfer(socket: UdpSocket, client_addr: std::net::SocketAddr) -> io::Result<()> {
    println!("Image transfer handler started on {}", socket.local_addr()?);

    let mut buf = [0; 1024 + 2];
    let mut received_packets: HashMap<u16, Vec<u8>> = HashMap::new();
    let mut total_packets = 0;

    // Step 1: Receive the image from the client
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

    let temp_file = format!("temp_image_{}.jpeg", socket.local_addr()?);
    let mut file = File::create(&temp_file)?;
    file.write_all(&image_data)?;
    println!("Image saved as '{}'.", temp_file);

    // Step 2: Send the image back to the client
    let mut file = File::open(&temp_file)?;
    let mut image_data = Vec::new();
    file.read_to_end(&mut image_data)?;

    let max_packet_size = 1022;
    let mut packet_number: u16 = 0;

    for chunk in image_data.chunks(max_packet_size) {
        let mut packet = Vec::with_capacity(2 + chunk.len());
        packet.extend_from_slice(&packet_number.to_be_bytes());
        packet.extend_from_slice(chunk);

        loop {
            socket.send_to(&packet, client_addr).await?;
            println!("Sent packet {}", packet_number);

            let mut ack_buf = [0; 2];
            match tokio::time::timeout(Duration::from_secs(1), socket.recv_from(&mut ack_buf)).await {
                Ok(Ok((_, _))) => {
                    let ack_packet_number = u16::from_be_bytes(ack_buf);
                    if ack_packet_number == packet_number {
                        println!("Acknowledgment received for packet {}", packet_number);
                        break;
                    }
                }
                _ => {
                    println!("No acknowledgment received for packet {}, resending...", packet_number);
                }
            }
        }

        packet_number += 1;
    }

    let terminator = [255, 255];
    socket.send_to(&terminator, client_addr).await?;
    println!("Image sent back to client.");

    Ok(())
}