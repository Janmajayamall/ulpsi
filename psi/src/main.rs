use std::collections::HashMap;

use bfv::{EvaluationKey, Evaluator, SecretKey};
use itertools::Itertools;
use psi::{
    construct_query, db, deserialize_query_response, gen_bfv_params, gen_random_item_labels,
    process_query_response, serialize_query_response, PsiParams, Server,
};
use rand::thread_rng;

fn main() {
    let psi_params = PsiParams::default();
    let mut server = Server::new(&psi_params);

    let set_size = 100;
    let raw_item_labels = gen_random_item_labels(set_size);

    server.setup(&raw_item_labels);

    server.print_diagnosis();

    // client chooses random values from raw_item_labels and constructs query set
    let mut expected_item_label_map = HashMap::new();
    let query_set = raw_item_labels
        .iter()
        .take(1)
        .map(|il| {
            expected_item_label_map.insert(il.item(), il.label());
            il.item()
        })
        .collect_vec();

    let mut rng = thread_rng();

    let bfv_params = gen_bfv_params(&psi_params);
    let evaluator = Evaluator::new(bfv_params);
    let sk = SecretKey::random_with_params(evaluator.params(), &mut rng);
    let ek = EvaluationKey::new(evaluator.params(), &sk, &[0], &[], &[], &mut rng);

    let client_query_state = construct_query(&query_set, &psi_params, &evaluator, &sk, &mut rng);

    let query_response = server.query(client_query_state.query(), &ek);

    {
        let serialized_query_response =
            serialize_query_response(&query_response, evaluator.params());
        let query_response_back =
            deserialize_query_response(&serialized_query_response, &psi_params, &evaluator);

        assert_eq!(&query_response, &query_response_back);
    }

    let response = process_query_response(
        &psi_params,
        client_query_state.hash_tables(),
        &evaluator,
        &sk,
        &query_response,
    );

    println!(
        "Hash stack size: {}",
        client_query_state.hash_table_stack().len()
    );

    // remove items that were not inserted in any of the hash tables
    client_query_state
        .hash_table_stack()
        .iter()
        .for_each(|entry| {
            expected_item_label_map.remove(&entry.entry_value());
        });

    // check that all items and their labels are in response
    expected_item_label_map.iter().for_each(|(item, label)| {
        response.iter().for_each(|res| {
            if *item == res.item() {
                // label must exist
                assert!(res.labels().contains(label));
            }
        });
    });
}
