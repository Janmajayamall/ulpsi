use std::collections::HashMap;

use bfv::{EvaluationKey, Evaluator, SecretKey};
use itertools::Itertools;
use psi::{
    construct_query, db, gen_bfv_params, gen_random_item_labels, process_query_response, PsiParams,
    Server,
};
use rand::thread_rng;

fn main() {
    let psi_params = PsiParams::default();
    let mut server = Server::new(&psi_params);

    let set_size = 10;
    let raw_item_labels = gen_random_item_labels(set_size);

    server.setup(&raw_item_labels);

    // client chooses random values from raw_item_labels and constructs query set
    let mut expected_item_label_map = HashMap::new();
    let query_set = raw_item_labels
        .iter()
        .take(5)
        .map(|il| {
            expected_item_label_map.insert(il.0, il.1);
            il.0
        })
        .collect_vec();

    let mut rng = thread_rng();

    let bfv_params = gen_bfv_params(&psi_params);
    let evaluator = Evaluator::new(bfv_params);
    let sk = SecretKey::random_with_params(evaluator.params(), &mut rng);
    let ek = EvaluationKey::new(evaluator.params(), &sk, &[0], &[], &[], &mut rng);

    let client_query_state = construct_query(&query_set, &psi_params, &evaluator, &sk, &mut rng);

    let query_response = server.query(client_query_state.query(), &ek);

    let response = process_query_response(
        &psi_params,
        client_query_state.hash_tables(),
        &evaluator,
        &sk,
        &query_response,
    );

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
