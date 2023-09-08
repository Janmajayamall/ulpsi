use crate::{
    client::{HashTableQueryCts, Query},
    hash::Cuckoo,
    poly_interpolate::newton_interpolate,
    server::paterson_stockmeyer::ps_evaluate_poly,
    utils::{calculate_ps_powers_with_dag, construct_dag, gen_bfv_params, Node},
    PsiParams,
};
use bfv::{Ciphertext, EvaluationKey, Evaluator, Plaintext, Representation};
use crypto_bigint::{Encoding, U256};
use db::{BigBox, InnerBox};
use itertools::{izip, Itertools};
use ndarray::Array2;
use serde::{de::Visitor, Deserialize, Serialize};
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
    pub(crate) psi_pt_bits: u32,
    pub(crate) psi_pt_bytes: u32,
    pub(crate) bfv_pt_bits: u32,
    pub(crate) bfv_pt_bytes: u32,
    pub(crate) bfv_pt: u32,
}

impl PsiPlaintext {
    pub fn new(psi_pt_bits: u32, bfv_pt_bits: u32, bfv_pt: u32) -> PsiPlaintext {
        assert!(bfv_pt_bits.is_power_of_two() && bfv_pt_bits >= 8);
        assert!(psi_pt_bits.is_power_of_two() && psi_pt_bits >= 8);

        PsiPlaintext {
            psi_pt_bits,
            psi_pt_bytes: psi_pt_bits / 8,
            bfv_pt_bits,
            bfv_pt_bytes: bfv_pt_bits / 8,
            bfv_pt,
        }
    }

    pub fn slots_required(&self) -> u32 {
        // both are power of 2
        self.psi_pt_bytes / self.bfv_pt_bytes
    }

    pub fn bytes_per_chunk(&self) -> u32 {
        self.bfv_pt_bytes
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
#[derive(Clone, Debug, PartialEq)]
pub struct ItemLabel {
    item: U256,
    label: U256,
}
impl ItemLabel {
    pub fn new(item: U256, label: U256) -> ItemLabel {
        ItemLabel { item, label }
    }

    pub fn item(&self) -> &U256 {
        &self.item
    }

    pub fn label(&self) -> &U256 {
        &self.label
    }

    /// `item` is greater
    ///
    /// TODO: Switch this to an iterator
    pub fn get_chunk_at_index(&self, chunk_index: u32, psi_pt: &PsiPlaintext) -> (u32, u32) {
        let bytes_per_chunk = psi_pt.bytes_per_chunk();
        let bytes_to_skip = (chunk_index * bytes_per_chunk) as usize;

        let item_chunk_bytes =
            &self.item().to_le_bytes()[bytes_to_skip..bytes_to_skip + bytes_per_chunk as usize];
        let label_chunk_bytes =
            &self.label().to_le_bytes()[bytes_to_skip..bytes_to_skip + bytes_per_chunk as usize];
        (
            bytes_to_u32(&item_chunk_bytes),
            bytes_to_u32(&label_chunk_bytes),
        )
    }
}

impl Serialize for ItemLabel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut v = self.item().to_le_bytes().to_vec();
        v.extend(self.label().to_le_bytes().iter());
        serializer.serialize_bytes(&v)
    }
}

struct ItemLabelVisitor;

impl<'de> Visitor<'de> for ItemLabelVisitor {
    type Value = ItemLabel;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("struct ItemLabel")
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // must have 64 byte, 32 for item and 32 for label
        // if v.len() != 64 {
        //     return serde::de::Error::invalid_length(v.len(), &self);
        // }
        assert_eq!(v.len(), 64);

        let mut item_bytes = [0u8; 32];
        let mut label_bytes = [0u8; 32];

        for i in 0..32 {
            item_bytes[i] = v[i];
            label_bytes[i] = v[i + 32];
        }

        let item = U256::from_le_bytes(item_bytes);
        let label = U256::from_le_bytes(label_bytes);

        Ok(ItemLabel { item, label })
    }
}

impl<'de> Deserialize<'de> for ItemLabel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_byte_buf(ItemLabelVisitor)
    }
}

pub fn bytes_to_u32(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0u32, |acc, (index, byte)| {
        let acc = acc + ((*byte as u32) << (index * 8));
        acc
    })
}

pub struct Server {
    db: Db,
    powers_dag: HashMap<usize, Node>,
    psi_params: PsiParams,
    evaluator: Evaluator,
}

impl Server {
    pub fn psi_params(&self) -> &PsiParams {
        &self.psi_params
    }

    pub fn evaluator(&self) -> &Evaluator {
        &self.evaluator
    }

    pub fn new(psi_params: &PsiParams) -> Server {
        let evaluator = Evaluator::new(gen_bfv_params(psi_params));
        let powers_dag = construct_dag(&psi_params.source_powers, psi_params.ps_params.powers());

        let db = Db::new(psi_params);

        Server {
            powers_dag,
            db,
            psi_params: psi_params.clone(),
            evaluator,
        }
    }

    pub fn setup(&mut self, item_labels: &[ItemLabel]) {
        item_labels.iter().for_each(|(i)| {
            if self.db.insert(i) {
                // println!("Item {} inserted", i.item());
            } else {
                println!("Item {} insert failed. Duplicate Item.", i.item());
            }
        });

        self.db.preprocess();
    }

    pub fn query(&self, query: &Query, ek: &EvaluationKey) -> QueryResponse {
        self.db
            .handle_query(query, &self.evaluator, ek, &self.powers_dag)
    }

    pub fn print_diagnosis(&self) {
        self.db.print_diagnosis();
    }
}
#[cfg(test)]
mod tests {
    use rand::thread_rng;

    use crate::{bytes_to_u32, random_u256, ItemLabel};

    #[test]
    fn test_byte_to_u32() {
        let bytes = vec![49, 255];
        let v = bytes_to_u32(&bytes);
        dbg!(v);
    }

    #[test]
    fn serialise_and_deserialise_item_label() {
        let mut rng = thread_rng();
        let item = random_u256(&mut rng);
        let label = random_u256(&mut rng);

        let item_label = ItemLabel::new(item, label);

        let bytes = bincode::serialize(&item_label).unwrap();
        let item_label_back: ItemLabel = bincode::deserialize(&bytes).unwrap();

        assert_eq!(item_label, item_label_back);
    }
}
