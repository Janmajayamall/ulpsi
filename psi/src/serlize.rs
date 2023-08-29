use bfv::{
    BfvParameters, Ciphertext, CiphertextProto, Encoding, Evaluator, Representation, SecretKey,
};
use itertools::Itertools;
use prost::Message;
use rand::thread_rng;
use traits::TryFromWithParameters;

use crate::{HashTableQuery, HashTableQueryCts, PsiParams, Query};

pub fn size_of_seeded_ciphertext(evaluator: &Evaluator) -> usize {
    let mut rng = thread_rng();
    let m = vec![];
    let sk = SecretKey::random_with_params(evaluator.params(), &mut rng);
    let ct = evaluator.encrypt(
        &sk,
        &evaluator.plaintext_encode(&m, Encoding::default()),
        &mut rng,
    );
    let ct_proto = CiphertextProto::try_from_with_parameters(&ct, evaluator.params());
    ct_proto.encode_to_vec().len()
}

pub fn serialize_query(query: &Query, bfv_params: &BfvParameters) -> Vec<u8> {
    query
        .0
        .iter()
        .flat_map(|ht_query_cts| {
            ht_query_cts.0.iter().flat_map(|ct| {
                let ct_proto = CiphertextProto::try_from_with_parameters(ct, bfv_params);
                ct_proto.encode_to_vec()
            })
        })
        .collect_vec()
}

pub fn deserialize_query(bytes: &[u8], psi_params: &PsiParams, evaluator: &Evaluator) -> Query {
    // validate
    let size_single_ct = size_of_seeded_ciphertext(evaluator);

    // Query should have 1 HashTableQuery for each BigBox. Each HashTableQuery must have 1 InnerBoxQuery for each segment in its corresponding BigBox. A single InnerBoxQuery is a vector of ciphertext, where initial query is raised to all source powers.
    let expected_bytes = size_single_ct
        * psi_params.source_powers.len()
        * HashTableQuery::segments_count(
            &psi_params.ht_size,
            &psi_params.ct_slots,
            &psi_params.psi_pt,
        ) as usize
        * psi_params.no_of_hash_tables as usize;
    assert_eq!(bytes.len(), expected_bytes);

    let bytes_in_single_ht_query = HashTableQuery::segments_count(
        &psi_params.ht_size,
        &psi_params.ct_slots,
        &psi_params.psi_pt,
    ) as usize
        * psi_params.source_powers.len()
        * size_single_ct;
    let bytes_in_single_inner_box_query_all_powers =
        size_single_ct * psi_params.source_powers.len();
    // process each HashTableQuery
    let ht_query_cts = bytes
        .chunks_exact(bytes_in_single_ht_query)
        .map(|bytes_ht_query| {
            // process each InnerBoxQuery (raised to source powers) within HashTableQuery
            let ht_query_cts = bytes_ht_query
                .chunks_exact(bytes_in_single_inner_box_query_all_powers)
                .flat_map(|bytes_inner_box_query_all_powers| {
                    // process each power ciphertext
                    bytes_inner_box_query_all_powers
                        .chunks_exact(size_single_ct)
                        .map(|bytes_ct| {
                            let ct_proto = CiphertextProto::decode(bytes_ct).unwrap();
                            Ciphertext::try_from_with_parameters(&ct_proto, evaluator.params())
                        })
                })
                .collect_vec();
            HashTableQueryCts(ht_query_cts)
        })
        .collect();

    Query(ht_query_cts)
}
