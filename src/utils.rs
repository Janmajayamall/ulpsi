use crate::{server::paterson_stockmeyer::PSParams, PsiParams};
use bfv::{
    BfvParameters, Ciphertext, Encoding, EvaluationKey, Evaluator, Plaintext, PolyCache, PolyType,
    Representation, SecretKey,
};
use itertools::{izip, Itertools};
use rand::{distributions::Uniform, thread_rng, Rng};
use rand_chacha::rand_core::le;
use std::collections::HashMap;
use traits::TryEncodingWithParameters;

pub fn rtg_indices_and_levels(degree: usize) -> (Vec<isize>, Vec<usize>) {
    let mut rtg_levels = vec![];
    let mut rtg_indices = vec![];

    let level = 0usize;
    // inner sum
    let degree_by_2 = (degree >> 1) as isize;
    let mut i = 1;
    while i < degree_by_2 {
        rtg_indices.push(i);
        rtg_levels.push(level);
        i *= 2;
    }
    // row swap
    rtg_indices.push((2 * degree - 1) as isize);
    rtg_levels.push(level);

    // inner sum covers rot by 1, but just adding it here for levelled implementation
    rtg_indices.push(1);
    rtg_levels.push(level);

    (rtg_indices, rtg_levels)
}

pub fn decrypt_and_print(
    evaluator: &Evaluator,
    sk: &SecretKey,
    ct: &Ciphertext,
    tag: &str,
    m_start: usize,
    m_end: usize,
) {
    let noise = evaluator.measure_noise(sk, ct);
    let m = &evaluator.plaintext_decode(&evaluator.decrypt(sk, ct), Encoding::default())
        [m_start..m_end];
    println!("{tag} - Noise: {noise}; m[{m_start}..{m_end}]: {:?}", m);
}

pub struct Node {
    target: usize,
    depth: usize,
    s1: usize,
    s2: usize,
}

pub fn construct_dag(source_powers: &[usize], target_powers: &[usize]) -> HashMap<usize, Node> {
    let mut dag = HashMap::<usize, Node>::new();
    let mut max_depth = 0;

    for source in source_powers.iter() {
        dag.insert(
            source.clone(),
            Node {
                target: source.clone(),
                depth: 0,
                s1: 0,
                s2: 0,
            },
        );
    }

    for target in target_powers.iter() {
        if source_powers.contains(target) {
            continue;
        }

        let mut optimal_depth = target - 1;
        let mut optimal_s1 = target - 1;
        let mut optimal_s2 = 1;

        for s1 in target_powers.iter() {
            if s1 > target {
                continue;
            }

            let s2 = target - s1;
            if !dag.contains_key(&s2) {
                continue;
            }

            let depth_s1 = dag.get(&s1).unwrap().depth;
            let depth_s2 = dag.get(&s2).unwrap().depth;
            let depth = std::cmp::max(depth_s1, depth_s2) + 1;

            if depth < optimal_depth {
                optimal_depth = depth;
                optimal_s1 = s1.clone();
                optimal_s2 = s2;
            }
        }

        if max_depth < optimal_depth {
            max_depth = optimal_depth;
        }

        dag.insert(
            target.clone(),
            Node {
                target: target.clone(),
                depth: optimal_depth,
                s1: optimal_s1,
                s2: optimal_s2,
            },
        );
    }

    dbg!(max_depth);

    dag
}

pub fn calculate_ps_powers_with_dag(
    evaluator: &Evaluator,
    ek: &EvaluationKey,
    source_cts: &[Ciphertext],
    source_powers: &[usize],
    target_powers: &[usize],
    dag: &HashMap<usize, Node>,
    ps_params: &PSParams,
) -> HashMap<usize, Ciphertext> {
    let mut target_powers_cts = HashMap::new();

    // insert all source powers
    izip!(source_powers.iter(), source_cts.iter()).for_each(|(p, ct)| {
        target_powers_cts.insert(*p, ct.clone());
    });

    // calculate target powers from the respective source powers
    target_powers.iter().for_each(|p| {
        if !target_powers_cts.contains_key(p) {
            let node = dag.get(&p).unwrap();

            let op1 = target_powers_cts.get(&node.s1).expect("Source 1 missing");
            let op2 = target_powers_cts.get(&node.s2).expect("Source 2 missing");
            let mut power_ct = evaluator.mul(op1, op2);
            power_ct = evaluator.relinearize(&power_ct, ek);
            // insert target power
            target_powers_cts.insert(*p, power_ct);
        }
    });

    // convert all powers <= low_degree to `Evaluation` for efficient plaintext multiplication
    for i in 0..ps_params.low_degree() {
        let power = i + 1;

        match target_powers_cts.get_mut(&power) {
            Some(ct) => {
                evaluator.ciphertext_change_representation(ct, Representation::Evaluation);
            }
            _ => {}
        }
    }

    target_powers_cts
}

pub fn bfv_setup_test() -> (Evaluator, SecretKey) {
    let mut rng = thread_rng();
    let params = BfvParameters::default(3, 1 << 13);
    let sk = SecretKey::random_with_params(&params, &mut rng);

    (Evaluator::new(params), sk)
}

pub fn gen_bfv_params(psi_params: &PsiParams) -> BfvParameters {
    let mut params = BfvParameters::new(
        &psi_params.bfv_moduli,
        psi_params.bfv_plaintext,
        psi_params.bfv_degree,
    );
    params.enable_hybrid_key_switching(&[50, 50, 50]);
    params
}

pub fn gen_random_item_labels(count: usize) -> Vec<(u128, u128)> {
    let rng = thread_rng();
    rng.clone()
        .sample_iter(Uniform::new(0, u128::MAX))
        .take(count * 2)
        .zip(
            rng.clone()
                .sample_iter(Uniform::new(0, u128::MAX))
                .take(count * 2),
        )
        .map(|(item, label)| (item, label))
        .collect_vec()
}

pub fn value_to_chunks(value: u128, chunk_count: u32, bits_per_chunk: u32) -> Vec<u32> {
    let mask = (1 << bits_per_chunk) - 1;

    let mut chunks = vec![];
    for i in 0..chunk_count {
        chunks.push(((value >> (i * bits_per_chunk)) & mask) as u32)
    }
    chunks
}

/// Chunks must be in little endian
pub fn chunks_to_value(chunks: &[u32], total_bits: u32, bits_per_chunk: u32) -> u128 {
    assert!(chunks.len() == (total_bits / bits_per_chunk) as usize);

    let mut value = 0u128;

    chunks.iter().enumerate().for_each(|(index, c)| {
        value += (*c as u128) << (index * bits_per_chunk as usize);
    });

    value
}
#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use crate::client::calculate_source_powers;

    use super::*;

    #[test]
    fn dag() {
        let source_powers = vec![1, 3, 11, 18, 45, 225];
        let target_degree = 1304;
        let ps_low_deg = 44;
        let ps_params = PSParams::new(ps_low_deg, target_degree);
        construct_dag(&source_powers, ps_params.powers());
    }

    #[test]
    fn calculate_ps_powers_with_dag_works() {
        let source_powers = vec![1, 3, 11, 18, 45, 225];
        let target_degree = 1304;
        let ps_low_deg = 44;
        let ps_params = PSParams::new(ps_low_deg, target_degree);
        let dag = construct_dag(&source_powers, ps_params.powers());

        let mut rng = thread_rng();
        let (evaluator, sk) = bfv_setup_test();
        let ek = EvaluationKey::new(evaluator.params(), &sk, &[0], &[], &[], &mut rng);

        // we stick with a single input value spanned across all rows
        let input_value = 5;
        let input_vec = vec![input_value; evaluator.params().degree];
        let input_source_powers = calculate_source_powers(
            &input_vec,
            &source_powers,
            evaluator.params().plaintext_modulus as u32,
        );

        let input_source_powers_cts = input_source_powers
            .iter()
            .map(|i| {
                let pt = Plaintext::try_encoding_with_parameters(
                    i.as_slice(),
                    evaluator.params(),
                    Encoding::simd(0, PolyCache::None),
                );
                evaluator.encrypt(&sk, &pt, &mut rng)
            })
            .collect_vec();

        let target_power_cts = calculate_ps_powers_with_dag(
            &evaluator,
            &ek,
            &input_source_powers_cts,
            &source_powers,
            ps_params.powers(),
            &dag,
            &ps_params,
        );

        // check all target powers are correct
        ps_params.powers().iter().for_each(|power| {
            let power_ct = target_power_cts.get(power).unwrap();
            // dbg!(evaluator.measure_noise(&sk, &power_ct));
            let m =
                evaluator.plaintext_decode(&evaluator.decrypt(&sk, power_ct), Encoding::default());

            // calculate expected target power of input_value
            let expected_m = evaluator
                .params()
                .plaintext_modulus_op
                .exp(input_value as u64, *power);

            assert_eq!(m, vec![expected_m; evaluator.params().degree]);
        })
    }
}
