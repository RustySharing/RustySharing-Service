use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use std::sync::Arc;
use tokio::time::{self, Duration};
use std::process::Command;

pub async fn heartbeat_monitor(socket: Arc<Mutex<UdpSocket>>, leader_addr: String) {
    let mut interval = time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let heartbeat_msg = b"heartbeat";
        if let Err(e) = socket.lock().await.send_to(heartbeat_msg, &leader_addr).await {
            eprintln!("Failed to send heartbeat to {}: {}", leader_addr, e);
        }
    }
}

pub async fn heartbeat_receiver(socket: Arc<Mutex<UdpSocket>>, has_token: Arc<Mutex<bool>>) {
    let mut buf = [0; 1024];
    loop {
        let (len, addr) = match socket.lock().await.recv_from(&mut buf).await {
            Ok((len, addr)) => (len, addr),
            Err(e) => {
                eprintln!("Failed to receive heartbeat: {}", e);
                // on fail initiate a leader election
                initiate_leader_election(socket.clone(), vec![], "".to_string(), has_token.clone()).await;
                continue;
            }
        };

        if len == 9 && &buf[..9] == b"heartbeat" {
            println!("Received heartbeat from {}", addr);
            let mut token = has_token.lock().await;
            *token = true;
        }
    }
}

pub async fn initiate_leader_election(socket: Arc<Mutex<UdpSocket>>, server_list: Vec<String>, my_addr: String, has_token: Arc<Mutex<bool>>) -> String {
    let my_cpu_time = get_cpu_time();
    let my_index = server_list.iter().position(|x| x == &my_addr).unwrap();
    let prev_server_addr = &server_list[(my_index + server_list.len() - 1) % server_list.len()];
    let next_server_addr = &server_list[(my_index + 1) % server_list.len()];

    let prev_cpu_time = query_cpu_time(socket.clone(), prev_server_addr).await;
    let next_cpu_time = query_cpu_time(socket.clone(), next_server_addr).await;

    let new_leader_addr = if my_cpu_time > prev_cpu_time && my_cpu_time > next_cpu_time {
        my_addr.clone()
    } else if prev_cpu_time > next_cpu_time {
        prev_server_addr.clone()
    } else {
        next_server_addr.clone()
    };

    println!("Server {} is the new leader", new_leader_addr);

    // Send the token to the new leader
    let mut token = has_token.lock().await;
    *token = false;
    socket.lock().await
        .send_to(&[99], format!("{}:{}", new_leader_addr, 9001))
        .await
        .expect("Failed to send token to new leader");

    new_leader_addr
}

async fn query_cpu_time(socket: Arc<Mutex<UdpSocket>>, server_addr: &str) -> u64 {
    let query_msg = b"cpu_time";
    if let Err(e) = socket.lock().await.send_to(query_msg, server_addr).await {
        eprintln!("Failed to send CPU time query to {}: {}", server_addr, e);
    }

    let mut buf = [0; 1024];
    match socket.lock().await.recv_from(&mut buf).await {
        Ok((len, _)) => {
            let cpu_time_str = String::from_utf8_lossy(&buf[..len]);
            cpu_time_str.parse::<u64>().unwrap_or(0)
        }
        Err(e) => {
            eprintln!("Failed to receive CPU time from {}: {}", server_addr, e);
            0
        }
    }
}

fn get_cpu_time() -> u64 {
    let output = Command::new("sh")
        .arg("-c")
        .arg("cat /proc/stat | grep '^cpu ' | awk '{print $2+$3+$4+$5+$6+$7+$8}'")
        .output()
        .expect("Failed to execute command");

    let cpu_time_str = String::from_utf8_lossy(&output.stdout);
    cpu_time_str.trim().parse::<u64>().unwrap_or(0)
}
