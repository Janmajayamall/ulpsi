use std::vec;
use tokio::net::{TcpListener, TcpStream};

/// Set to some very large value (10 Mb)
const BUFFER_BYTES: usize = 10485760;

fn build_query() {}

#[tokio::main]
async fn main() {
    let stream = TcpStream::connect("127.0.0.1:6379").await.unwrap();

    // send request
    let mut bytes = (0..100).into_iter().collect::<Vec<u8>>();
    stream
        .try_write(&mut bytes)
        .expect("Failed to send request");

    // read response
    let mut response_buffer = vec![0u8; BUFFER_BYTES];

    loop {
        stream.readable().await.expect("Response failed");

        match stream.try_read(&mut response_buffer) {
            Ok(bytes) => {
                response_buffer.truncate(bytes);
            }
            Err(_) => {
                panic!("Could not read response")
            }
        }
    }
}
