use bfv::{EvaluationKey, EvaluationKeyProto};
use clap::{Parser, Subcommand};
use prost::Message;
use psi::{
    db::{self, Db},
    deserialize_query, expected_query_bytes, gen_random_item_labels,
    generate_random_intersection_and_store, serialize_query_response, ItemLabel, PsiParams, Server,
};
use std::{
    error::Error,
    io::{BufReader, BufWriter, Read},
};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, Result};
use tokio::net::{TcpListener, TcpStream};
use traits::TryFromWithParameters;

pub fn read_client_evaluation_key(server: &Server) -> Result<EvaluationKey> {
    let mut file = std::fs::File::open("./../data/client/client_evaluation_key.bin")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let ek_proto = EvaluationKeyProto::decode(&*buffer)?;
    let evaluation_key =
        EvaluationKey::try_from_with_parameters(&ek_proto, server.evaluator().params());
    Ok(evaluation_key)
}

/// Randomly generates `count` ItemLabels as server and stores them under directory ./data/{count}/server_set.bin
fn generate_random_server_set(count: usize) {
    // check server_set.bin already exists at necessary path. If it does, abort
    let dir_path = format!("./../data/{}", count);
    let mut server_set_file_path = PathBuf::from(dir_path.clone());
    server_set_file_path.push("server_set.bin");
    if Path::exists(&server_set_file_path) {
        panic!(
            "Server dataset for {} already exists at {}",
            count,
            server_set_file_path.display()
        );
    }

    let server_set = gen_random_item_labels(count);

    std::fs::create_dir_all(dir_path.clone())
        .expect(&format!("Creating directory at {} failed", dir_path));

    // rust does not uses buffered I/O by default. Use BufWriter to use buffered I/O.
    // Ref - https://stackoverflow.com/questions/49983101/serialization-of-large-struct-to-disk-with-serde-and-bincode-is-slow
    let mut server_file = BufWriter::new(
        File::create(server_set_file_path).expect("Failed to create server_set.bin"),
    );
    bincode::serialize_into(&mut server_file, &server_set).unwrap();
}

/// Runs preprocessing for server using server set stored at `dir_path`/server_set.bin (for ex, data/1000/server_set.bin). Then stores pre-processed server's `Db` at `dir_path`/server_db_preprocessed.bin.
fn preprocess_and_store_dataset(dir_path: &Path, psi_params: &PsiParams) {
    // check that preprocessed data already exists. If it does then abort
    let mut server_db_preprocessed_path = PathBuf::from(dir_path);
    server_db_preprocessed_path.push("server_db_preprocessed.bin");
    if Path::exists(&server_db_preprocessed_path) {
        panic!(
            "server_db_preprocessed.bin file already exists at {}",
            server_db_preprocessed_path.display()
        );
    }

    // read server set
    let mut server_set_path = PathBuf::from(dir_path);
    server_set_path.push("server_set.bin");
    let file = std::fs::File::open(server_set_path.clone()).expect(&format!(
        "Failed to open server_set.bin at {}",
        server_set_path.display()
    ));
    let reader = BufReader::new(file);
    let item_labels: Vec<ItemLabel> =
        bincode::deserialize_from(reader).expect("Invalid server_set.bin file");

    println!(
        "Preprocessing server set with {} ItemLabels",
        item_labels.len()
    );

    // create new server and setup
    let mut server = Server::new(psi_params);
    server.setup(&item_labels);
    server.print_diagnosis();

    // serialize and store server db in server_db_preprocessed.bin
    let mut server_db_preprocessed_file =
        BufWriter::new(std::fs::File::create(server_db_preprocessed_path).unwrap());
    bincode::serialize_into(&mut server_db_preprocessed_file, server.db()).unwrap();
}

/// Returns an active instance of `Server` by loading preprocessed server db file stored at `server_db_preprocessed`
fn load_server(server_db_preprocessed: &Path, psi_params: &PsiParams) -> Server {
    let file = std::fs::File::open(server_db_preprocessed.clone()).expect(&format!(
        "Failed to open server_db_preprocessed.bin at {}",
        server_db_preprocessed.display()
    ));
    let reader = BufReader::new(file);
    let db: Db = bincode::deserialize_from(reader).expect(&format!(
        "Malformed server db bin file {}",
        server_db_preprocessed.display()
    ));

    Server::new_with_db(db, psi_params)
}

/// Loads server_set.bin stored at `dir_path`/server_set.bin and randomly generates client_set of `intersection_size`. Stores the client set at `dir_path/client_set.bin`.
fn generate_random_client_intersection_set(intersection_size: usize, dir_path: &Path) {
    let mut server_set_path = PathBuf::from(dir_path);
    server_set_path.push("server_set.bin");

    let mut client_set_path = PathBuf::from(dir_path);
    client_set_path.push("client_set.bin");

    let file = std::fs::File::open(server_set_path.clone()).expect(&format!(
        "Failed to open server_set.bin at {}",
        server_set_path.display()
    ));
    let reader = BufReader::new(file);
    let item_labels: Vec<ItemLabel> = bincode::deserialize_from(reader).expect(&format!(
        "Malformed server set bin file {}",
        server_set_path.display()
    ));

    let client_set = generate_random_intersection_and_store(&item_labels, intersection_size);
    assert_eq!(client_set.len(), intersection_size);

    let mut client_set_file =
        BufWriter::new(File::create(client_set_path).expect("Failed to create client_set.bin"));
    bincode::serialize_into(&mut client_set_file, &client_set).unwrap();
}

/// Starts the server using server_set.bin and server_db_preprocessed.bin stored inside `dir_path` directory.
async fn start_server(dir_path: &Path) {
    let psi_params = PsiParams::default();

    let mut server_db_preprocessed_path = PathBuf::from(dir_path);
    server_db_preprocessed_path.push("server_db_preprocessed.bin");

    println!("Loading server db state in memory...");
    let server = load_server(&server_db_preprocessed_path, &psi_params);
    server.print_diagnosis();

    // Bind the listener to the address
    let addr = "127.0.0.1:6379";
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("Server started. Listening on {}", addr);

    loop {
        // The second item contains the IP and port of the new connection.
        let (mut socket, _) = listener.accept().await.unwrap();
        match process_query(socket, &server).await {
            Ok(_) => {
                println!("Request returned successfully!");
                println!();
            }
            Err(e) => {
                println!("Request failed with error: {e}");
                println!();
            }
        }
    }
}

async fn process_query(mut socket: TcpStream, server: &Server) -> Result<()> {
    socket.readable().await?;

    println!("Received New Query");

    // read query into buffer
    let expected_bytes = expected_query_bytes(server.evaluator(), server.psi_params());
    let mut query_buffer = vec![0; expected_bytes];
    socket.read_exact(&mut query_buffer).await?;

    // deserialize query
    println!("Deserializing Query...");
    let query = deserialize_query(&query_buffer, server.psi_params(), server.evaluator());

    // read client's evaluation key
    println!("Deserializing Client Evaluation Key...");
    let client_evaluation_key = read_client_evaluation_key(server)?;

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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    // #[arg(short, long)]
    // debug: u8,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Setup {
        set_size: usize,
    },
    Preprocess {
        set_size: usize,
    },
    Start {
        set_size: usize,
    },
    GenClientSet {
        server_set_size: usize,
        client_set_size: usize,
    },
}

fn set_size_to_dir_path(set_size: usize) -> PathBuf {
    let dir_path = PathBuf::from(&format!("./../data/{}", set_size));
    dir_path
}

#[tokio::main]
async fn main() {
    // generate_random_dataset(16000000);
    // let psi_params = PsiParams::default();
    // preprocess_and_store_dataset(Path::new("./../data/16000000"), &psi_params);
    // // start_server().await;
    // return;
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { set_size } => {
            start_server(&set_size_to_dir_path(set_size)).await;
        }
        Commands::Preprocess { set_size } => {
            let psi_params = PsiParams::default();
            preprocess_and_store_dataset(&set_size_to_dir_path(set_size), &psi_params);
        }
        Commands::Setup { set_size } => {
            let dir_path = set_size_to_dir_path(set_size);
            let psi_params = PsiParams::default();
            generate_random_server_set(set_size);
            preprocess_and_store_dataset(&dir_path, &psi_params);
        }
        Commands::GenClientSet {
            server_set_size,
            client_set_size,
        } => {
            generate_random_client_intersection_set(
                client_set_size,
                &set_size_to_dir_path(server_set_size),
            );
        }
    }
}
