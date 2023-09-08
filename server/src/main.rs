use bfv::{EvaluationKey, EvaluationKeyProto};
use prost::Message;
use psi::{
    db, deserialize_query, expected_query_bytes, serialize_query_response, ItemLabel, PsiParams,
    Server,
};
use std::{error::Error, io::Read};
use tokio::{
    io::{AsyncBufReadExt, BufReader, *},
    net::{TcpListener, TcpStream},
};
use traits::TryFromWithParameters;

#[tokio::main]
async fn main() {
    let psi_params = PsiParams::default();
    let mut server = Server::new(&psi_params);

    // setup server
    let file =
        std::fs::File::open("./../data/server_set.bin").expect("Failed to open server_set.bin");
    let reader = std::io::BufReader::new(file);
    let item_labels: Vec<ItemLabel> =
        bincode::deserialize_from(reader).expect("Invalid server_set.bin file");
    // setup server with item labels
    server.setup(&item_labels);
    server.print_diagnosis();

    // Bind the listener to the address
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();

    loop {
        // The second item contains the IP and port of the new connection.
        let (mut socket, _) = listener.accept().await.unwrap();
        match process(socket, &server).await {
            Ok(_) => {
                println!("Request returned successfully!")
            }
            Err(e) => {
                println!("Request failed with error: {e}")
            }
        }
    }
}

pub fn read_client_evaluation_key(server: &Server) -> Result<EvaluationKey> {
    let mut file = std::fs::File::open("./../data/client_evaluation_key.bin")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let ek_proto = EvaluationKeyProto::decode(&*buffer)?;
    let evaluation_key =
        EvaluationKey::try_from_with_parameters(&ek_proto, server.evaluator().params());
    Ok(evaluation_key)
}

async fn process(mut socket: TcpStream, server: &Server) -> Result<()> {
    socket.readable().await?;

    // read query into buffer
    let expected_bytes = expected_query_bytes(server.evaluator(), server.psi_params());
    let mut query_buffer = vec![0; expected_bytes];
    socket.read_exact(&mut query_buffer).await?;

    // deserialize query
    let query = deserialize_query(&query_buffer, server.psi_params(), server.evaluator());
    println!("Deserialize Query");

    // read client's evaluation key
    let client_evaluation_key = read_client_evaluation_key(server)?;
    println!("Deserialize Client Evaluation Key");

    // Start processing Query
    println!("Processing Query...");
    let now = std::time::Instant::now();
    let query_response = server.query(&query, &client_evaluation_key);
    println!("Query Processing Time: {} ms", now.elapsed().as_millis());

    // serialize response
    let serialized_query_response =
        serialize_query_response(&query_response, server.evaluator().params());

    let response_bytes = bincode::serialize(&serialized_query_response).unwrap();

    socket.writable().await?;

    socket.write_all(&response_bytes).await?;

    Ok(())
}
