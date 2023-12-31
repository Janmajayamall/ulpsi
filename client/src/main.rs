use bfv::{BfvParameters, EvaluationKey, EvaluationKeyProto, Evaluator, SecretKey, SecretKeyProto};
use crypto_bigint::U256;
use prost::Message;
use psi::{
    construct_query, db, deserialize_query_response, gen_bfv_params, generate_evaluation_key,
    process_query_response, serialize_query, ItemLabel, PsiParams, SerializedQueryResponse,
};
use rand::thread_rng;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::{error::Error, io::BufReader};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use traits::TryFromWithParameters;

fn generate_random_client_with_evaluation_key_and_store(
    evaluator: &Evaluator,
) -> (SecretKey, EvaluationKey) {
    let mut rng = thread_rng();
    let sk = SecretKey::random_with_params(evaluator.params(), &mut rng);
    let ek = generate_evaluation_key(&evaluator, &sk);

    // serliaze keys
    let sk_serliazed = SecretKeyProto::try_from_with_parameters(&sk, evaluator.params());
    let mut sk_bytes = sk_serliazed.encode_to_vec();

    let ek_serliazed = EvaluationKeyProto::try_from_with_parameters(&ek, evaluator.params());
    let mut ek_bytes = ek_serliazed.encode_to_vec();

    // store sk and ek for server
    let client_dir = "./../data/client";
    let mut client_sk_path = PathBuf::from(client_dir);
    client_sk_path.push("client_secret_key.bin");
    let mut client_ek_path = PathBuf::from(client_dir);
    client_ek_path.push("client_evaluation_key.bin");
    std::fs::create_dir_all(client_dir).expect("Create data directory failed");
    let mut sk_file =
        std::fs::File::create(client_sk_path).expect("Failed to create client_secret_key.bin");
    sk_file
        .write_all(&mut sk_bytes)
        .expect("Failed to write client_secret_key.bin");

    let mut ek_file =
        std::fs::File::create(client_ek_path).expect("Failed to create client_evaluation_key.bin");
    ek_file
        .write_all(&mut ek_bytes)
        .expect("Failed to write client_evaluation_key.bin");

    (sk, ek)
}

pub fn read_client_secret_key(bfv_params: &BfvParameters) -> SecretKey {
    let mut file = std::fs::File::open("./../data/client_secret_key.bin")
        .expect("Failed to open client_secret_key.bin");
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .expect("Unable to read client_secret_key.bin");
    let proto = SecretKeyProto::decode(&*buffer).expect("Malformed client_secret_key.bin");
    let secret_key = SecretKey::try_from_with_parameters(&proto, &bfv_params);
    secret_key
}

pub async fn simulate_query(client_set_path: &Path) {
    let psi_params = PsiParams::default();
    let bfv_params = gen_bfv_params(&psi_params);
    let evaluator = Evaluator::new(bfv_params);

    println!("Reading Client Set...");
    let file = std::fs::File::open(client_set_path).expect(&format!(
        "Failed to open client set at {}",
        client_set_path.display()
    ));
    let reader = BufReader::new(file);
    let item_labels: Vec<ItemLabel> =
        bincode::deserialize_from(reader).expect("Invalid client set file");

    println!("Generating random client secret key and evaluation key...");
    let (client_secret_key, _) = generate_random_client_with_evaluation_key_and_store(&evaluator);

    println!("Constructing query...");
    let mut rng = thread_rng();
    let query_set = item_labels
        .iter()
        .map(|il| il.item().clone())
        .collect::<Vec<U256>>();
    let query_state = construct_query(
        &query_set,
        &psi_params,
        &evaluator,
        &client_secret_key,
        &mut rng,
    );

    // serialize query
    let mut serialized_query = serialize_query(query_state.query(), evaluator.params());

    println!("Query Size: {} Bytes", serialized_query.len());

    // send request
    println!("Sending query...");
    let mut stream = TcpStream::connect("127.0.0.1:6379").await.unwrap();

    stream
        .write_all(&mut serialized_query)
        .await
        .expect("Failed to send query request");
    stream.flush().await.expect("A");

    // read response
    let mut response_buffer = Vec::new();

    stream
        .readable()
        .await
        .expect("Failed to read response from server");
    stream
        .read_to_end(&mut response_buffer)
        .await
        .expect("Failed to read response from server");

    let serialized_query_response: SerializedQueryResponse =
        bincode::deserialize(&response_buffer).unwrap();
    let query_response =
        deserialize_query_response(&serialized_query_response, &psi_params, &evaluator);

    println!("Query Response Size: {} Bytes", response_buffer.len());

    // validate query response
    let response = process_query_response(
        &psi_params,
        query_state.hash_tables(),
        &evaluator,
        &client_secret_key,
        &query_response,
    );

    // check all item labels are present
    item_labels.iter().for_each(|il| {
        // if item_label is in hash table stack, then ignore it.
        let mut in_stack_flag = false;
        query_state.hash_table_stack().iter().for_each(|ht_entry| {
            if il.item() == ht_entry.entry_value() {
                in_stack_flag = true;
            }
        });

        if !in_stack_flag {
            // find the item in response and check that label exists as one of the potential response labels
            response.iter().for_each(|res| {
                if res.item() == il.item() {
                    assert!(res.labels().contains(&il.label()));
                }
            })
        }
    });

    println!("Query Success!");
}

#[tokio::main]
async fn main() {
    let client_set_path = std::env::args()
        .nth(1)
        .expect("Pass path to client intersection set");

    simulate_query(Path::new(&client_set_path)).await;
}
