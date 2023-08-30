use crate::{
    db, HashTableQuery, HashTableQueryCts, HashTableQueryResponse, PsiParams, Query, QueryResponse,
};
use bfv::{
    BfvParameters, Ciphertext, CiphertextProto, Encoding, Evaluator, PolyCache, Representation,
    SecretKey,
};
use itertools::Itertools;
use prost::Message;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use traits::TryFromWithParameters;

#[derive(Serialize, Deserialize)]
pub struct SerializedQueryResponse {
    // TODO: check response size with and without `serde_bytes`
    #[serde(with = "serde_bytes")]
    bytes: Vec<u8>,
    /// indicates no. of inner boxes within a segment. Segments of each bigbox are stored in continuation.
    inner_boxes_per_segment: Vec<usize>,
}

pub fn size_of_unseeded_ciphertext_last_level(evaluator: &Evaluator) -> usize {
    let mut rng = thread_rng();
    let m = vec![];
    let sk = SecretKey::random_with_params(evaluator.params(), &mut rng);
    let mut ct = evaluator.encrypt(
        &sk,
        &evaluator.plaintext_encode(&m, Encoding::default()),
        &mut rng,
    );

    // nullify seed
    evaluator.ciphertext_change_representation(&mut ct, Representation::Evaluation);
    let pt = evaluator.plaintext_encode(&m, Encoding::simd(0, PolyCache::Mul(bfv::PolyType::Q)));
    evaluator.mul_plaintext_assign(&mut ct, &pt);

    // mod down to last level
    evaluator.ciphertext_change_representation(&mut ct, Representation::Coefficient);
    evaluator.mod_down_level(&mut ct, evaluator.params().ciphertext_moduli.len() - 1);

    let ct_proto = CiphertextProto::try_from_with_parameters(&ct, evaluator.params());
    ct_proto.encode_to_vec().len()
}

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

pub fn expected_query_bytes(evaluator: &Evaluator, psi_params: &PsiParams) -> usize {
    let size_single_ct = size_of_seeded_ciphertext(evaluator);
    size_single_ct
        * psi_params.source_powers.len()
        * HashTableQuery::segments_count(
            &psi_params.ht_size,
            &psi_params.ct_slots,
            &psi_params.psi_pt,
        ) as usize
        * psi_params.no_of_hash_tables as usize
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

pub fn serialize_query_response(
    query_response: &QueryResponse,
    bfv_params: &BfvParameters,
) -> SerializedQueryResponse {
    let bytes = query_response
        .0
        .iter()
        .flat_map(|ht_query_response| {
            ht_query_response.0.iter().flat_map(|segment_response_cts| {
                segment_response_cts.iter().flat_map(|ct| {
                    let ct_proto = CiphertextProto::try_from_with_parameters(ct, bfv_params);
                    let tmp = ct_proto.encode_to_vec();
                    tmp
                })
            })
        })
        .collect_vec();

    let inner_box_lengths = query_response
        .0
        .iter()
        .flat_map(|ht_query_response| {
            ht_query_response
                .0
                .iter()
                .map(|segment_response_cts| segment_response_cts.len())
        })
        .collect_vec();

    SerializedQueryResponse {
        bytes,
        inner_boxes_per_segment: inner_box_lengths,
    }
}

pub fn deserialize_query_response(
    serialized_query_response: &SerializedQueryResponse,
    psi_params: &PsiParams,
    evaluator: &Evaluator,
) -> QueryResponse {
    // Can't validate bytes directly since response size is variable.
    let bytes_single_ct = size_of_unseeded_ciphertext_last_level(evaluator);

    let segments_per_hash_table = HashTableQuery::segments_count(
        &psi_params.ht_size,
        &psi_params.ct_slots,
        &psi_params.psi_pt,
    ) as usize;
    let total_expected_segments_response =
        psi_params.no_of_hash_tables as usize * segments_per_hash_table;
    assert_eq!(
        serialized_query_response.inner_boxes_per_segment.len(),
        total_expected_segments_response
    );

    let mut query_response = vec![];
    let mut ciphertexts_processed = 0;
    serialized_query_response
        .inner_boxes_per_segment
        .chunks_exact(segments_per_hash_table)
        .for_each(|segments| {
            // process segments of BigBox
            let mut ht_table_query_response = vec![];
            segments.iter().for_each(|segment_length| {
                // process response ciphertexts for the segment
                let mut segment_query_response = vec![];
                for inner_box_index in 0..*segment_length {
                    let bytes = &serialized_query_response.bytes[ciphertexts_processed
                        * bytes_single_ct
                        ..(ciphertexts_processed + 1) * bytes_single_ct];
                    let ct_proto = CiphertextProto::decode(bytes).unwrap();
                    let ct = Ciphertext::try_from_with_parameters(&ct_proto, evaluator.params());
                    segment_query_response.push(ct);
                    ciphertexts_processed += 1;
                }
                ht_table_query_response.push(segment_query_response);
            });

            query_response.push(HashTableQueryResponse(ht_table_query_response));
        });

    QueryResponse(query_response)
}
