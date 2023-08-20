use std::{collections::HashMap, ops::Deref};

use bfv::{Ciphertext, Encoding, Evaluator, Modulus, Plaintext, SecretKey};
use itertools::{izip, Itertools};
use rand::{CryptoRng, Rng, RngCore};
use traits::TryEncodingWithParameters;

use crate::{
    hash::{self, construct_hash_tables, Cuckoo, HashTableEntry},
    server::{CiphertextSlots, HashTableSize, PsiPlaintext},
    PsiParams,
};

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

        let value_chunks = self.value_chunks(entry.entry_value());
        for i in real_row..(real_row + self.psi_pt.slots_required()) {
            self.data[i as usize] = value_chunks[(i - real_row) as usize];
        }
    }

    pub fn max_rows(ct_slots: &CiphertextSlots, psi_pt: &PsiPlaintext) -> u32 {
        ct_slots.deref() / psi_pt.slots_required()
    }
}

/// Processed by server on BigBox
pub struct HashTableQuery {
    ib_queries: Vec<InnerBoxQuery>,
    ht_size: HashTableSize,
    psi_pt: PsiPlaintext,
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

                    // which segement (ie ib_query) to inset into
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
}

/// Encrypted queries for the HashTable. Contains 2D array of ciphertext where a single row
/// contains same InnerBoxQuery raised to required source powers. There must be as many as `Segments`
/// rows, one InnerBoxQuery for each segment of BigBox.
pub struct HashTableQueryCts(pub(crate) Vec<Vec<Ciphertext>>);

pub struct Query(pub(crate) Vec<HashTableQueryCts>);

fn construct_query<R: RngCore + CryptoRng>(
    query_set: &[u128],
    psi_params: &PsiParams,
    evaluator: &Evaluator,
    sk: &SecretKey,
    rng: &mut R,
) -> Query {
    let ht_entries = query_set
        .iter()
        .map(|q| HashTableEntry::new(*q))
        .collect_vec();

    let cuckoo = &Cuckoo::new(psi_params.no_of_hash_tables, *psi_params.ht_size.deref());

    // Each hash table returned is a hash map storing values under key equivalent to respective index.
    let (hash_tables, stack) = construct_hash_tables(&ht_entries, &cuckoo);

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

    Query(ht_queries_cts)
}

#[cfg(test)]
mod tests {
    // fn calculate_source_powers_works()
}
