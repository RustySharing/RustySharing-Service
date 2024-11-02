use tokio::net::UdpSocket;
use std::fs::File;
use std::io::Write;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    let mut buf = vec![0; 1024 + 2]; // Buffer for receiving data (packet size + 2 bytes for sequence number)
    let mut received_packets: HashMap<u16, Vec<u8>> = HashMap::new();
    let mut total_packets = 0;

    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;

        // Check for the end of transmission signal
        if len == 2 && buf[0] == 255 && buf[1] == 255 {
            println!("End of transmission signal received.");
            break;
        }

        // Extract the packet number from the first 2 bytes
        let packet_number = u16::from_be_bytes([buf[0], buf[1]]);
        let data = buf[2..len].to_vec();

        // Store the packet data
        received_packets.insert(packet_number, data);
        total_packets = total_packets.max(packet_number + 1);

        // Send acknowledgment back to the client
        socket.send_to(&packet_number.to_be_bytes(), addr).await?;
        println!("Received packet {} and sent acknowledgment.", packet_number);
    }

    // Reassemble the image in order
    let mut image_data = Vec::new();
    for i in 0..total_packets {
        if let Some(chunk) = received_packets.remove(&i) {
            image_data.extend(chunk);
        }
    }

    // Save the image data to a file
    let mut file = File::create("received_image.jpeg")?;
    file.write_all(&image_data)?;
    println!("Image saved as 'received_image.jpeg'.");

    Ok(())
}