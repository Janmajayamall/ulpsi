use std::{collections::HashMap, hash::Hash};

use bfv::{
    BfvParameters, Ciphertext, Encoding, EvaluationKey, Evaluator, Plaintext, PolyCache, PolyType,
    Representation, SecretKey,
};
use rand::thread_rng;
use rand_chacha::rand_core::le;
use utils::{decrypt_and_print, rtg_indices_and_levels};

mod data;
mod poly_interpolate;
mod utils;

struct PsiParams {
    item_size: usize,
    label_size: usize,
    /// Max. no. of items stores at server
    server_item_count: usize,
    /// Max. no. of items client can request at once
    client_item_count: usize,

    // BFV params
    bfv_degree: usize,
    plaintext_bits: usize,
}

impl PsiParams {
    /// No. of lanes that a single item occupies
    fn lanes_per_item(&self) -> usize {
        (self.item_size + self.plaintext_bits) / self.plaintext_bits
    }
}

/// Warning: We assume that bits in both label and item are equal.
struct ItemLabel(u32, u32);
impl ItemLabel {
    pub fn item(&self) -> u32 {
        self.0
    }

    pub fn label(&self) -> u32 {
        self.1
    }

    /// `item` is greater
    ///
    /// TODO: Switch this to an iterator
    pub fn get_chunk_at_index(&self, chunk_index: usize, psi_pt: &PsiPlaintext) -> (u32, u32) {
        let bits = psi_pt.chunk_bits();
        let mask = (1 << bits) - 1;

        (
            self.item() >> (chunk_index as u32 * bits) & mask,
            self.label() >> (chunk_index as u32 * bits) & mask,
        )
    }
}

#[derive(Clone)]
struct PsiPlaintext(u64, u64);
impl PsiPlaintext {
    fn chunk_bits(&self) -> u32 {
        64 - self.1.leading_zeros() - 1
    }
}

struct PolyCoefficientsCache {
    coefficients_2d: Vec<u32>,
    label_size: usize,
    psi_plaintext: PsiPlaintext,
}

impl PolyCoefficientsCache {
    // constructs polynomials with degree len(iterm_labels). Divdes each label in
    // `label_size/plaintext_bit_space` parts and interpolates a polynomial for each.
    pub fn new(item_labels: &[ItemLabel], psi_plaintext: &PsiPlaintext) -> PolyCoefficientsCache {
        todo!()
    }
}

struct Db {
    data: HashMap<u32, Vec<ItemLabel>>,
    hash_table_size: u32,
    lane_per_item: u32,
    poly_coeff_cache: HashMap<u32, Vec<PolyCoefficientsCache>>,
}

impl Db {
    fn insert(&mut self, item_label: ItemLabel) {
        //TODO use the hash function to hash
        let item_hash = 0 % self.hash_table_size;

        if self.data.contains_key(&item_hash) {
            self.data.get_mut(&item_hash).unwrap().push(item_label);
        } else {
            let values = vec![item_label];
            self.data.insert(item_hash, values);
        }
    }

    fn iterpolate(&self) {}
}

struct Server {
    raw_label_data: Vec<u32>,
    raw_item_data: Vec<u32>,
}

fn to_power(x: &Ciphertext, evaluator: &Evaluator, ek: &EvaluationKey, power: usize) -> Ciphertext {
    assert!(power.is_power_of_two());
    let mut curr_power = 1;
    let mut x = x.clone();
    while curr_power != power {
        x = evaluator.mul(&x, &x);
        x = evaluator.relinearize(&x, ek);
        curr_power *= 2;
    }
    x
}

fn equality(
    evaluator: &Evaluator,
    ek: &EvaluationKey,
    x: &Ciphertext,
    values: &Plaintext,
    sk: &SecretKey,
) -> Ciphertext {
    let mut x = evaluator.sub_plaintext(&x, &values);

    // `to_power` modifies i^th lane value to 1 if the lane value > 0. Otherwise value remains unchanged.
    x = to_power(
        &x,
        evaluator,
        ek,
        (evaluator.params().plaintext_modulus - 1) as usize,
    );

    // decrypt_and_print(evaluator, sk, &x, "is_equal", 0, 10);

    // sub from 1
    evaluator.negate_assign(&mut x);
    let one = evaluator.plaintext_encode(
        &vec![1; evaluator.params().degree],
        Encoding::simd(0, PolyCache::AddSub(Representation::Coefficient)),
    );
    evaluator.add_assign_plaintext(&mut x, &one);

    x
}

fn inner_sum(x: &Ciphertext, evaluator: &Evaluator, ek: &EvaluationKey) -> Ciphertext {
    let degree_by_2 = ((evaluator.params().degree) >> 1) as isize;
    let mut i = 1;

    let mut x = x.clone();
    while i < degree_by_2 {
        let tmp = evaluator.rotate(&x, i, ek);
        evaluator.add_assign(&mut x, &tmp);
        i *= 2;
    }

    // row swap
    let tmp = evaluator.rotate(&x, (2 * evaluator.params().degree - 1) as isize, ek);
    evaluator.add_assign(&mut x, &tmp);
    x
}

fn multiplication_tree(cts: &mut [Ciphertext], evaluator: &Evaluator, ek: &EvaluationKey) {
    assert!(cts.len().is_power_of_two());
    let mut depth = (cts.len() as f64).log2();

    let mut step = 1;
    while depth > 0.0 {
        let mut i = 1;
        while i < cts.len() {
            cts[i - 1] = evaluator.relinearize(&evaluator.mul(&cts[i - 1], &cts[i + step - 1]), ek);
            i += (step * 2);
        }

        depth -= 1.0;
        step *= 2;
    }
}

/// Extracts `slot_count` lanes starting from `offset` into individuals ciphertexts. Takes care of extracting
/// each value in same lane in new ciphertexts. Multiplies extract ciphertext into one, expands the result, and returns
fn extract_tag_slots_and_return_pv(
    evaluator: &Evaluator,
    ek: &EvaluationKey,
    x: &Ciphertext,
    slot_count: usize,
    offset: usize,
    sk: &SecretKey,
) -> Ciphertext {
    // offset must be multiple of slot_count
    assert!(offset % slot_count == 0);
    assert!(x.c_ref()[0].representation() == &Representation::Evaluation); // for plaintext multiplication

    let mut m = vec![0; evaluator.params().degree];
    m[offset] = 1;
    let pt = evaluator.plaintext_encode(&m, Encoding::simd(0, PolyCache::Mul(PolyType::Q)));

    // Values correspoding to a single tag is spread across `slot_count` lanes. Here we extract value in each lane into its own ciphertext resulting in `slot_count` ciphertexts.
    // All values are extracted to same lane (which is lane at `offset` index) so that ciphertexts can be multiplied together to generate pertinency bit for the tag.
    let mut x = x.clone();
    let mut extracted_slots = vec![];
    for i in 0..slot_count {
        // extract current lane
        extracted_slots.push(evaluator.mul_plaintext(&x, &pt));
        // rotate left by 1, to extract next value in next iteration
        if i != slot_count - 1 {
            x = evaluator.rotate(&x, 1, ek);
        }
    }

    // multiply extracted slot into single ciphertext to generate pertinency bit
    multiplication_tree(&mut extracted_slots, evaluator, ek);
    dbg!(extracted_slots.len());

    // inner_sum (due to rotations) is more efficient in Evluation representration. Change representation
    // of product (stored at index 0) to Evaluation
    evaluator.ciphertext_change_representation(&mut extracted_slots[0], Representation::Evaluation);

    // expand pertinency bit across all lanes, resulting into pertinency vector.
    // Pertinency vector indicates whether tag
    let pv = inner_sum(&extracted_slots[0], evaluator, ek);

    pv
}

#[cfg(test)]
mod tests {
    use bfv::Representation;
    use itertools::Itertools;
    use rand::Rng;

    use super::*;

    #[test]
    fn run() {
        let mut params = BfvParameters::new(&[50; 15], 65537, 1 << 14);
        params.enable_hybrid_key_switching(&[50, 50, 50]);

        let mut rng = thread_rng();

        // gen keys
        let sk = SecretKey::random_with_params(&params, &mut rng);
        let (rtg_indices, rtg_levels) = rtg_indices_and_levels(params.degree);
        let ek = EvaluationKey::new(&params, &sk, &[0], &rtg_levels, &rtg_indices, &mut rng);

        let evaluator = Evaluator::new(params);

        let m = evaluator
            .params()
            .plaintext_modulus_op
            .random_vec(evaluator.params().degree, &mut rng);
        let pt = evaluator.plaintext_encode(&m, Encoding::default());
        let ct = evaluator.encrypt(&sk, &pt, &mut rng);

        let pt2 = evaluator.plaintext_encode(
            &m,
            Encoding::simd(0, PolyCache::AddSub(Representation::Coefficient)),
        );

        // equality
        let mut r_ct = equality(&evaluator, &ek, &ct, &pt2, &sk);
        evaluator.ciphertext_change_representation(&mut r_ct, Representation::Evaluation);
        let pv_ct = extract_tag_slots_and_return_pv(&evaluator, &ek, &r_ct, 16, 16, &sk);
        dbg!(evaluator.measure_noise(&sk, &pv_ct));

        decrypt_and_print(&evaluator, &sk, &pv_ct, "pv_ct", 0, 10);
        // dbg!(res_m);
    }

    #[test]
    fn equality_works() {
        let mut params = BfvParameters::new(&[50; 15], 65537, 1 << 14);
        params.enable_hybrid_key_switching(&[50, 50, 50]);

        let mut rng = thread_rng();

        // gen keys
        let sk = SecretKey::random_with_params(&params, &mut rng);
        let ek = EvaluationKey::new(&params, &sk, &[0], &[], &[], &mut rng);

        let evaluator = Evaluator::new(params);

        let mut m0 = evaluator
            .params()
            .plaintext_modulus_op
            .random_vec(evaluator.params().degree, &mut rng);
        let mut m1 = m0.clone();

        // select random indices and make them different in m0 and m1
        let mut diff_indices = vec![];
        while diff_indices.len() != 100 {
            let v = rng.gen_range(0..=evaluator.params().degree);
            if !diff_indices.contains(&v) {
                diff_indices.push(v);
            }
        }
        diff_indices.iter().for_each(|i| {
            m1[*i] = m0[*i] + 1;
        });

        let ct = evaluator.encrypt(
            &sk,
            &evaluator.plaintext_encode(&m0, Encoding::default()),
            &mut rng,
        );
        let pt1 = evaluator.plaintext_encode(
            &m1,
            Encoding::simd(0, PolyCache::AddSub(Representation::Coefficient)),
        );

        let res = equality(&evaluator, &ek, &ct, &pt1, &sk);
        let res_m = evaluator.plaintext_decode(&evaluator.decrypt(&sk, &res), Encoding::default());
        // all indices, except the ones in `diff_indices`, are equal. Thus
        // `equality` should return ciphertext with 1 at all indices except the ones
        // in `diff_indices` where value must be 0.
        let mut expected_res = vec![1; evaluator.params().degree];
        diff_indices.iter().for_each(|i| {
            expected_res[*i] = 0;
        });

        assert_eq!(res_m, expected_res);
    }

    #[test]
    fn extract_tag_slots_works() {
        let mut params = BfvParameters::new(&[50; 15], 65537, 1 << 14);
        params.enable_hybrid_key_switching(&[50, 50, 50]);

        let mut rng = thread_rng();

        // gen keys
        let sk = SecretKey::random_with_params(&params, &mut rng);
        let (rtg_indices, rtg_levels) = rtg_indices_and_levels(params.degree);
        let ek = EvaluationKey::new(&params, &sk, &[0], &rtg_levels, &rtg_indices, &mut rng);

        let evaluator = Evaluator::new(params);

        let mut m = vec![1; evaluator.params().degree];
        let slot_count = 16;

        // Think of lanes divided in sets of `slot_count`. pertinency vector corresponding to each set will be 1 only when
        // all lanes in the set are 1, otherwise it will be 0.
        // We intentionally set set 1 and 3 to have pertinency vector as 0.
        m[slot_count - 1] = 0; // set 1
        m[slot_count * 2 + 1] = 0; // set 2

        let mut x = evaluator.encrypt(
            &sk,
            &evaluator.plaintext_encode(&m, Encoding::default()),
            &mut rng,
        );
        evaluator.ciphertext_change_representation(&mut x, Representation::Evaluation);

        // `pv` of set 1,3 should be 0 and 1 for rest of the sets
        // set 1
        let pv1 = extract_tag_slots_and_return_pv(&evaluator, &ek, &x, slot_count, 0, &sk);
        // set 3
        let pv3 =
            extract_tag_slots_and_return_pv(&evaluator, &ek, &x, slot_count, slot_count * 2, &sk);
        let pv1_m = evaluator.plaintext_decode(&evaluator.decrypt(&sk, &pv1), Encoding::default());
        let pv3_m = evaluator.plaintext_decode(&evaluator.decrypt(&sk, &pv3), Encoding::default());
        assert_eq!(pv1_m, pv3_m);
        assert_eq!(pv1_m, vec![0; evaluator.params().degree]);

        // `pv` for set 2,5 (and the rest) should be 1
        // set 2
        let pv2 =
            extract_tag_slots_and_return_pv(&evaluator, &ek, &x, slot_count, slot_count * 1, &sk);
        // set 5
        let pv5 =
            extract_tag_slots_and_return_pv(&evaluator, &ek, &x, slot_count, slot_count * 4, &sk);
        let pv2_m = evaluator.plaintext_decode(&evaluator.decrypt(&sk, &pv2), Encoding::default());
        let pv5_m = evaluator.plaintext_decode(&evaluator.decrypt(&sk, &pv5), Encoding::default());
        assert_eq!(pv2_m, pv5_m);
        assert_eq!(pv2_m, vec![1; evaluator.params().degree]);
    }

    #[test]
    fn multiplication_tree_works() {
        let mut params = BfvParameters::new(&[50; 10], 65537, 1 << 14);
        params.enable_hybrid_key_switching(&[50, 50, 50]);

        let mut rng = thread_rng();
        let sk = SecretKey::random_with_params(&params, &mut rng);
        let ek = EvaluationKey::new(&params, &sk, &[0], &[], &[], &mut rng);

        let evaluator = Evaluator::new(params);

        let length = 16;
        let mut cts = (1..(length + 1))
            .into_iter()
            .map(|i| {
                let pt = evaluator
                    .plaintext_encode(&vec![i; evaluator.params().degree], Encoding::default());
                evaluator.encrypt(&sk, &pt, &mut rng)
            })
            .collect_vec();

        multiplication_tree(&mut cts, &evaluator, &ek);
        let res = evaluator.plaintext_decode(&evaluator.decrypt(&sk, &cts[0]), Encoding::default());
        let mut expected_res = 1;
        (2..(length + 1)).for_each(|i| {
            expected_res *= i;
        });
        expected_res %= evaluator.params().plaintext_modulus;

        dbg!(evaluator.measure_noise(&sk, &cts[0]));
        assert_eq!(res, vec![expected_res; evaluator.params().degree]);
    }
}
