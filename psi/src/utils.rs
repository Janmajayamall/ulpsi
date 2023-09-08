use crate::{
    bytes_to_u32, db, random_u256,
    server::{paterson_stockmeyer::PSParams, ItemLabel},
    PsiParams,
};
use bfv::{
    BfvParameters, Ciphertext, EvaluationKey, Evaluator, Plaintext, PolyCache, PolyType,
    Representation, SecretKey,
};
use crypto_bigint::{Encoding, U256};
use itertools::{izip, Itertools};
use rand::{distributions::Uniform, thread_rng, Rng};
use rand_chacha::rand_core::le;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use std::collections::HashMap;
use traits::TryEncodingWithParameters;

pub fn decrypt_and_print(
    evaluator: &Evaluator,
    sk: &SecretKey,
    ct: &Ciphertext,
    tag: &str,
    m_start: usize,
    m_end: usize,
) {
    let noise = evaluator.measure_noise(sk, ct);
    let m = &evaluator.plaintext_decode(&evaluator.decrypt(sk, ct), bfv::Encoding::default())
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

/// Calculates target powers ciphertexts from source powers ciphertexts using DAG. All source powers ciphertexts
/// must be in Coefficient representation. Before returning all ciphertexts corresponding to power <= low_degree are changed
/// to Evaluation representation for efficient plaintext multiplication in inner k loop for PS.
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
        assert!(ct.c_ref()[0].representation() == &Representation::Coefficient);
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
    let psi_params = PsiParams::default();
    let mut params = BfvParameters::new(
        &psi_params.bfv_moduli,
        psi_params.bfv_plaintext,
        psi_params.bfv_degree,
    );
    params.enable_hybrid_key_switching(&psi_params.hybrid_ksk_moduli);
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

pub fn gen_random_item_labels(count: usize) -> Vec<ItemLabel> {
    let cores = rayon::current_num_threads();

    let count_per_thread = count / cores;
    let count_last_thread = (count - count_per_thread * cores) + count_per_thread;
    dbg!(cores);
    // Use up all cores.
    (0..cores)
        .into_par_iter()
        .flat_map(|core_index| {
            let take = if core_index == cores - 1 {
                count_last_thread
            } else {
                count_per_thread
            };
            let mut rng = thread_rng();
            (0..take)
                .into_iter()
                .map(|_| {
                    let item = random_u256(&mut rng);
                    let label = random_u256(&mut rng);
                    ItemLabel::new(item, label)
                })
                .collect_vec()
        })
        .collect()
}

pub fn value_to_chunks(value: &U256, no_of_chunks: u32, bytes_per_chunk: u32) -> Vec<u32> {
    let value_bytes = value.to_le_bytes();

    let mut chunks = vec![];
    for chunk_index in 0..no_of_chunks {
        let chunk_start = (chunk_index * bytes_per_chunk) as usize;
        let bytes = &value_bytes[(chunk_start..chunk_start + bytes_per_chunk as usize)];
        chunks.push(bytes_to_u32(bytes));
    }

    chunks
}

/// Chunks must be in little endian
pub fn chunks_to_value(chunks: &[u32], total_bytes: u32, bytes_per_chunk: u32) -> U256 {
    assert!(chunks.len() == (total_bytes / bytes_per_chunk) as usize);

    let mut u256_bytes = [0u8; 32];

    let mut byte_index = 0;
    chunks.iter().enumerate().for_each(|(_, c)| {
        (0..bytes_per_chunk).into_iter().for_each(|index| {
            // extract byte
            u256_bytes[byte_index] = ((c >> (index * 8)) & 255) as u8;
            byte_index += 1;
        });
    });

    U256::from_le_bytes(u256_bytes)
}

// Measures time in ms for enclosed code block.
// Credit: https://github.com/zama-ai/demo_z8z/blob/1f24eeaf006263543062e90f1d1692d381a726cf/src/zqz/utils.rs#L28C1-L42C2
#[macro_export]
macro_rules! time_it{
    ($title: tt, $($block:tt)+) => {
        let __now = std::time::SystemTime::now();
        $(
           $block
        )+
        let __time = __now.elapsed().unwrap().as_millis();
        let __ms_time = format!("{} ms", __time);
        println!("{} duration: {}", $title, __ms_time);
    }
}

pub fn generate_evaluation_key(evaluator: &Evaluator, sk: &SecretKey) -> EvaluationKey {
    let mut rng = thread_rng();
    EvaluationKey::new(evaluator.params(), &sk, &[0], &[], &[], &mut rng)
}

/// Generates random ItemLabels and stores them update /data dir. We store the file as .bin since it is the fastest.
fn generate_random_item_labels_and_store(set_size: usize) {
    let server_set = gen_random_item_labels(set_size);

    // // create parent directory for data
    std::fs::create_dir_all("./../data").expect("Create data directory failed");

    let mut server_file =
        std::fs::File::create("./../data/server_set.bin").expect("Failed to create server_set.bin");
    bincode::serialize_into(server_file, &server_set).unwrap();
}

fn generate_random_intersection_and_store(intersection_size: usize) {
    let server_set: Vec<ItemLabel> = bincode::deserialize_from(
        std::fs::File::open("./../data/server_set.bin").expect("Failed to open server_set.bin"),
    )
    .expect("Malformed server_set.bin");

    let set_size = server_set.len();

    assert!(set_size > intersection_size);

    let mut inserted_indices = vec![];
    let mut client_set = vec![];
    let mut rng = thread_rng();
    while inserted_indices.len() != intersection_size {
        let index = rng.gen_range(0..set_size);
        if !inserted_indices.contains(&index) {
            inserted_indices.push(index);
            client_set.push(server_set[index].clone());
        }
    }

    let mut client_file =
        std::fs::File::create("./../data/client_set.bin").expect("Failed to create client_set.bin");
    bincode::serialize_into(&client_file, &client_set).unwrap();
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
                    bfv::Encoding::simd(0, PolyCache::None),
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
            let m = evaluator
                .plaintext_decode(&evaluator.decrypt(&sk, power_ct), bfv::Encoding::default());

            // calculate expected target power of input_value
            let expected_m = evaluator
                .params()
                .plaintext_modulus_op
                .exp(input_value as u64, *power);

            assert_eq!(m, vec![expected_m; evaluator.params().degree]);
        })
    }

    #[test]
    fn prepare_random_data_big() {
        generate_random_item_labels_and_store(16000000);
        generate_random_intersection_and_store(3000);
    }
}
