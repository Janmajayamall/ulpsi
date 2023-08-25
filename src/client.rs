use std::{collections::HashMap, ops::Deref};

use bfv::{Ciphertext, Encoding, Evaluator, Modulus, Plaintext, SecretKey};
use itertools::{izip, Itertools};
use rand::{CryptoRng, Rng, RngCore};
use traits::{TryDecodingWithParameters, TryEncodingWithParameters};

use crate::{
    chunks_to_value,
    hash::{self, construct_hash_tables, Cuckoo, HashTableEntry},
    server::{db, CiphertextSlots, HashTableSize, PsiPlaintext},
    value_to_chunks, HashTableQueryResponse, PsiParams, QueryResponse,
};

#[derive(Debug, Clone)]
pub struct PotentialResponseLabels {
    pub(crate) item: u128,
    pub(crate) labels: Vec<u128>,
}

impl PotentialResponseLabels {
    pub fn item(&self) -> u128 {
        self.item
    }

    pub fn labels(&self) -> &[u128] {
        &self.labels
    }
}

/// Calculate source powers  for each element of input_vec and returns. Returns a 2d array where each column
/// corresponds input_vec elements raised to source power (in ascending order)
pub fn calculate_source_powers(
    input_vec: &[u32],
    source_powers: &[usize],
    modq: u32,
) -> Vec<Vec<u32>> {
    let modq = Modulus::new(modq as u64);

    let max_power = source_powers.iter().max().unwrap();
    let mut ouput_vec = vec![];
    let mut curr_input_vec = input_vec.to_vec();
    for p in 1..(*max_power + 1) {
        if (source_powers.contains(&p)) {
            ouput_vec.push(curr_input_vec.clone());
        }

        izip!(curr_input_vec.iter_mut(), input_vec.iter()).for_each(|(c, i)| {
            *c = modq.mul_mod_fast(*c as u64, *i as u64) as u32;
        });
    }

    ouput_vec
}

/// Processed by server on each segment (ie vectors of InnerBoxes correspoding to a subset of hash table rows)
pub struct InnerBoxQuery {
    data: Vec<u32>,
    psi_pt: PsiPlaintext,
}

impl InnerBoxQuery {
    pub fn new(ct_slots: &CiphertextSlots, psi_pt: &PsiPlaintext) -> InnerBoxQuery {
        let data = vec![0u32; *ct_slots.deref() as usize];
        InnerBoxQuery {
            data,
            psi_pt: psi_pt.clone(),
        }
    }

    pub fn value_chunks(&self, value: u128) -> Vec<u32> {
        let bits = self.psi_pt.chunk_bits();
        let mask = (1 << bits) - 1;

        let mut chunks = vec![];
        for i in 0..self.psi_pt.slots_required() {
            chunks.push(((value >> (i * bits)) & mask) as u32)
        }
        chunks
    }

    pub fn insert_entry(&mut self, row: u32, entry: &HashTableEntry) {
        let real_row = row * self.psi_pt.slots_required();

        let value_chunks = value_to_chunks(
            entry.entry_value(),
            self.psi_pt.slots_required(),
            self.psi_pt.chunk_bits(),
        );
        for i in real_row..(real_row + self.psi_pt.slots_required()) {
            self.data[i as usize] = value_chunks[(i - real_row) as usize];
        }
    }

    pub fn max_rows(ct_slots: &CiphertextSlots, psi_pt: &PsiPlaintext) -> u32 {
        ct_slots.deref() / psi_pt.slots_required()
    }

    pub fn process_segment_response_at_row(
        psi_pt: &PsiPlaintext,
        expected_row: u32,
        segment_response: &Vec<Vec<u32>>,
    ) -> Vec<u128> {
        let real_row = expected_row * psi_pt.slots_required();

        segment_response
            .iter()
            .map(|res| {
                let mut res_value_chunks = vec![];
                for i in real_row..(real_row + psi_pt.slots_required()) {
                    res_value_chunks.push(res[i as usize]);
                }
                let res_value =
                    chunks_to_value(&res_value_chunks, psi_pt.psi_pt_bits, psi_pt.chunk_bits());
                res_value
            })
            .collect_vec()
    }
}

/// Processed by server on BigBox
pub struct HashTableQuery {
    ib_queries: Vec<InnerBoxQuery>,
    ht_size: HashTableSize,
    psi_pt: PsiPlaintext,
    /// No. of rows in a single `InnerBox` query
    ib_query_rows: u32,
}

impl HashTableQuery {
    pub fn new(
        ht_size: &HashTableSize,
        ct_slots: &CiphertextSlots,
        psi_pt: &PsiPlaintext,
    ) -> HashTableQuery {
        let ib_query_rows = InnerBoxQuery::max_rows(ct_slots, psi_pt);
        let segments = (ht_size.deref() + (ib_query_rows >> 1)) / ib_query_rows;

        let ib_queries = (0..segments)
            .into_iter()
            .map(|_| InnerBoxQuery::new(ct_slots, psi_pt))
            .collect_vec();

        HashTableQuery {
            ib_queries,
            ht_size: ht_size.clone(),
            psi_pt: psi_pt.clone(),
            ib_query_rows,
        }
    }

    pub fn process_hash_table(&mut self, hash_table: &HashMap<u32, HashTableEntry>) {
        for i in 0..*self.ht_size.deref() {
            match hash_table.get(&i) {
                Some(entry) => {
                    // map i^th row to row in InnerBoxQuery
                    let ib_row = i % self.ib_query_rows;

                    // which segement (ie ib_query) to insert into
                    let segment_index = i / self.ib_query_rows;

                    // insert
                    self.ib_queries[segment_index as usize].insert_entry(ib_row, entry);
                }
                _ => {}
            }
        }
    }

    pub fn process_inner_box_queries_with_source_powers_and_encrypt<R: CryptoRng + RngCore>(
        &self,
        source_powers: &[usize],
        evaluator: &Evaluator,
        sk: &SecretKey,
        rng: &mut R,
    ) -> HashTableQueryCts {
        let ht_table_query_cts = self
            .ib_queries
            .iter()
            .map(|q| {
                let q_sources_powers = calculate_source_powers(
                    &q.data,
                    &source_powers,
                    evaluator.params().plaintext_modulus as u32,
                );

                // encrypt `q` raised to different source powers
                let q_source_powers_ct = q_sources_powers
                    .iter()
                    .map(|q_power| {
                        let pt = Plaintext::try_encoding_with_parameters(
                            q_power.as_slice(),
                            evaluator.params(),
                            Encoding::default(),
                        );

                        evaluator.encrypt(sk, &pt, rng)
                    })
                    .collect_vec();

                q_source_powers_ct
            })
            .collect_vec();

        HashTableQueryCts(ht_table_query_cts)
    }

    pub fn process_hash_table_query_response(
        psi_params: &PsiParams,
        evaluator: &Evaluator,
        sk: &SecretKey,
        hash_table: &HashMap<u32, HashTableEntry>,
        ht_query_response: &HashTableQueryResponse,
    ) -> Vec<PotentialResponseLabels> {
        // InnerBoxQuery is constructed per Segment
        let inner_box_max_rows = InnerBoxQuery::max_rows(&psi_params.ct_slots, &psi_params.psi_pt);
        let original_inner_box_queries =
            (psi_params.ht_size.0 + (inner_box_max_rows >> 1)) / inner_box_max_rows;

        // segments in response and in the query must be equal
        assert_eq!(
            ht_query_response.0.len(),
            original_inner_box_queries as usize
        );

        // decrypt responses
        let segment_responses = ht_query_response
            .0
            .iter()
            .map(|segment_cts| {
                segment_cts
                    .iter()
                    .map(|ct| {
                        let pt = evaluator.decrypt(sk, ct);
                        Vec::<u32>::try_decoding_with_parameters(
                            &pt,
                            evaluator.params(),
                            Encoding::default(),
                        )
                    })
                    .collect_vec()
            })
            .collect_vec();

        let mut response = vec![];
        for i in 0..*psi_params.ht_size.deref() {
            match hash_table.get(&i) {
                Some(entry) => {
                    // which segement do we expect the response to be in
                    let segment_index = i / inner_box_max_rows;

                    // response corresponding to segment contains multiple vectors, since a segment is further divided into
                    // multiple innerboxes.
                    let segment_response = &segment_responses[segment_index as usize];

                    let expected_ib_row = i % inner_box_max_rows;

                    let potential_responses = InnerBoxQuery::process_segment_response_at_row(
                        &psi_params.psi_pt,
                        expected_ib_row,
                        segment_response,
                    );

                    response.push(PotentialResponseLabels {
                        item: entry.entry_value(),
                        labels: potential_responses,
                    });
                }
                _ => {}
            }
        }

        response
    }
}

/// Encrypted queries for the HashTable. Contains 2D array of ciphertext where a single row
/// contains same InnerBoxQuery raised to required source powers. There must be as many as `Segments`
/// rows, one InnerBoxQuery for each segment of BigBox.
pub struct HashTableQueryCts(pub(crate) Vec<Vec<Ciphertext>>);

pub struct Query(pub(crate) Vec<HashTableQueryCts>);

pub struct QueryState {
    pub(crate) query: Query,
    pub(crate) hash_tables: Vec<HashMap<u32, HashTableEntry>>,
    pub(crate) hash_table_stack: Vec<HashTableEntry>,
}

impl QueryState {
    pub fn query(&self) -> &Query {
        &self.query
    }

    pub fn hash_tables(&self) -> &[HashMap<u32, HashTableEntry>] {
        &self.hash_tables
    }

    pub fn hash_table_stack(&self) -> &[HashTableEntry] {
        &self.hash_table_stack
    }
}

pub fn construct_query<R: RngCore + CryptoRng>(
    query_set: &[u128],
    psi_params: &PsiParams,
    evaluator: &Evaluator,
    sk: &SecretKey,
    rng: &mut R,
) -> QueryState {
    let ht_entries = query_set
        .iter()
        .map(|q| HashTableEntry::new(*q))
        .collect_vec();

    let cuckoo = &Cuckoo::new(psi_params.no_of_hash_tables, *psi_params.ht_size.deref());

    // Each hash table returned is a hash map storing values under key equivalent to respective index.
    let (hash_tables, stack) = construct_hash_tables(&ht_entries, &cuckoo);
    dbg!(stack.len());
    let ht_queries = hash_tables
        .iter()
        .map(|ht| {
            let mut ht_query = HashTableQuery::new(
                &psi_params.ht_size,
                &psi_params.ct_slots,
                &psi_params.psi_pt,
            );
            ht_query.process_hash_table(ht);
            ht_query
        })
        .collect_vec();

    // encrypt ht_queries
    let ht_queries_cts = ht_queries
        .iter()
        .map(|htq| {
            htq.process_inner_box_queries_with_source_powers_and_encrypt(
                &psi_params.source_powers,
                &evaluator,
                &sk,
                rng,
            )
        })
        .collect_vec();

    QueryState {
        query: Query(ht_queries_cts),
        hash_tables: hash_tables,
        hash_table_stack: stack,
    }
}

pub fn process_query_response(
    psi_params: &PsiParams,
    hash_table: &[HashMap<u32, HashTableEntry>],
    evaluator: &Evaluator,
    sk: &SecretKey,
    query_response: &QueryResponse,
) -> Vec<PotentialResponseLabels> {
    // QueryResponse must contain as many HashTableQueryResponse as there are HashTables
    assert_eq!(
        query_response.0.len(),
        psi_params.no_of_hash_tables as usize
    );

    println!("Ht responses {}", query_response.0.len());

    let ht_response = &query_response.0[0];
    println!("Ht responses segments {}", ht_response.0.len());

    // Process HashTableQueryResponse corresponding to each hash table
    let potential_response_labels = query_response
        .0
        .iter()
        .enumerate()
        .flat_map(|(ht_index, ht_response)| {
            HashTableQuery::process_hash_table_query_response(
                psi_params,
                evaluator,
                sk,
                &hash_table[ht_index],
                ht_response,
            )
        })
        .collect_vec();

    potential_response_labels
}

#[cfg(test)]
mod tests {
    use rand::{distributions::Uniform, thread_rng};

    use crate::utils::gen_bfv_params;

    use super::*;

    #[test]
    fn construct_query_works() {
        let mut rng = thread_rng();
        let psi_params = PsiParams::default();

        let bfv_params = gen_bfv_params(&psi_params);
        let evaluator = Evaluator::new(bfv_params);
        let sk = SecretKey::random_with_params(evaluator.params(), &mut rng);

        let query_set = rng
            .clone()
            .sample_iter(Uniform::new(0, u128::MAX))
            .take(100)
            .collect_vec();

        let query_response = construct_query(&query_set, &psi_params, &evaluator, &sk, &mut rng);
    }
}
