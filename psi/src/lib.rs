use std::{collections::HashMap, hash::Hash};

use bfv::{
    BfvParameters, Ciphertext, Encoding, EvaluationKey, Evaluator, Plaintext, PolyCache, PolyType,
    Representation, SecretKey, SecretKeyProto,
};
use hash::Cuckoo;
use itertools::{izip, Itertools};
use rand::thread_rng;
use rand_chacha::rand_core::le;
use server::{
    paterson_stockmeyer::PSParams, CiphertextSlots, EvalPolyDegree, HashTableSize, PsiPlaintext,
};

pub use client::*;
pub use hash::*;
pub use poly_interpolate::*;
pub use serlize::*;
pub use server::*;
pub use utils::*;

mod client;
mod hash;
mod poly_interpolate;
mod serlize;
mod server;
mod utils;

#[derive(Clone, Debug)]
pub struct PsiParams {
    pub(crate) no_of_hash_tables: u8,
    pub(crate) ht_size: HashTableSize,
    pub(crate) ct_slots: CiphertextSlots,
    pub(crate) eval_degree: EvalPolyDegree,
    pub(crate) bfv_moduli: Vec<usize>,
    pub(crate) bfv_degree: usize,
    pub(crate) bfv_plaintext: u64,
    pub(crate) psi_pt: PsiPlaintext,
    pub(crate) ps_params: PSParams,
    pub(crate) source_powers: Vec<usize>,
}

impl Default for PsiParams {
    fn default() -> Self {
        let ps_params = PSParams::new(44, 1304);
        let psi_pt = PsiPlaintext::new(256, 16, 65537);

        PsiParams {
            no_of_hash_tables: 3,
            ht_size: HashTableSize(1 << 12),
            ct_slots: CiphertextSlots(1 << 13),
            eval_degree: ps_params.eval_degree(),
            bfv_moduli: vec![50, 50, 50],
            bfv_degree: 1 << 13,
            bfv_plaintext: 65537,
            psi_pt,
            ps_params,
            source_powers: vec![1, 3, 11, 18, 45, 225],
        }
    }
}

#[cfg(test)]
mod tests {}
