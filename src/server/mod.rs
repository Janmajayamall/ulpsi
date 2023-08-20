use crate::{
    client::{HashTableQueryCts, Query},
    hash::Cuckoo,
    poly_interpolate::newton_interpolate,
    server::paterson_stockmeyer::ps_evaluate_poly,
    utils::{calculate_ps_powers_with_dag, construct_dag, Node},
    PsiParams,
};
use bfv::{Ciphertext, Encoding, EvaluationKey, Evaluator, Plaintext, Representation};
use db::{BigBox, InnerBox};
use itertools::{izip, Itertools};
use ndarray::Array2;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};

pub use db::*;

pub mod db;
pub mod paterson_stockmeyer;

/// No. of rows on a hash table
#[derive(Clone, Debug)]
pub struct HashTableSize(pub(crate) u32);

impl Deref for HashTableSize {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct PsiPlaintext {
    psi_pt_bits: u32,
    bfv_pt_bits: u32,
    bfv_pt: u32,
}

impl PsiPlaintext {
    pub fn new(psi_pt_bits: u32, bfv_pt_bits: u32, bfv_pt: u32) -> PsiPlaintext {
        PsiPlaintext {
            psi_pt_bits,
            bfv_pt_bits,
            bfv_pt,
        }
    }

    pub fn slots_required(&self) -> u32 {
        (self.psi_pt_bits + (self.bfv_pt_bits >> 1)) / self.bfv_pt_bits
    }

    pub fn chunk_bits(&self) -> u32 {
        self.bfv_pt_bits
    }
}

/// No. of slots in a single BFV ciphertext. Equivalent to degree of ciphertext.
#[derive(Clone, Debug)]
pub struct CiphertextSlots(pub(crate) u32);

impl Deref for CiphertextSlots {
    type Target = u32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Degree of interpolated polynomial
#[derive(Clone, Debug)]
pub struct EvalPolyDegree(u32);

impl EvalPolyDegree {
    /// InnerBox needs to have column capacity equivalent to no. of data
    /// points EvalPoly can interpolate. EvalPoly can interpolate `EvalPolyDegree + 1`
    /// data points
    pub fn inner_box_columns(&self) -> u32 {
        self.0 + 1
    }
}

/// Warning: We assume that bits in both label and item are equal.
pub struct ItemLabel(u128, u128);
impl ItemLabel {
    pub fn new(item: u128, label: u128) -> ItemLabel {
        ItemLabel(item, label)
    }

    pub fn item(&self) -> u128 {
        self.0
    }

    pub fn label(&self) -> u128 {
        self.1
    }

    /// `item` is greater
    ///
    /// TODO: Switch this to an iterator
    pub fn get_chunk_at_index(&self, chunk_index: u32, psi_pt: &PsiPlaintext) -> (u32, u32) {
        let bits = psi_pt.chunk_bits();
        let mask = (1 << bits) - 1;

        (
            ((self.item() >> (chunk_index * bits)) & mask) as u32,
            ((self.item() >> (chunk_index * bits)) & mask) as u32,
        )
    }
}

struct Server {
    db: Db,
    powers_dag: HashMap<usize, Node>,
    psi_params: PsiParams,
    evaluator: Evaluator,
}

impl Server {
    pub fn new(psi_params: &PsiParams, evaluator: Evaluator) -> Server {
        let powers_dag = construct_dag(&psi_params.source_powers, psi_params.ps_params.powers());

        let db = Db::new(psi_params);

        Server {
            powers_dag,
            db,
            psi_params: psi_params.clone(),
            evaluator,
        }
    }

    pub fn query(&self, query: &Query, ek: &EvaluationKey) {
        self.db
            .handle_query(query, &self.evaluator, ek, &self.powers_dag);
    }
}
#[cfg(test)]
mod tests {
    use rand::{thread_rng, Rng};

    use super::*;
}
