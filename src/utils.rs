use std::collections::HashMap;

use bfv::{
    BfvParameters, Ciphertext, Encoding, EvaluationKey, Evaluator, Plaintext, PolyCache, PolyType,
    Representation, SecretKey,
};
use rand::thread_rng;
use rand_chacha::rand_core::le;

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

struct Node {
    target: usize,
    depth: usize,
    s1: usize,
    s2: usize,
}

pub fn construct_dag(source_powers: &[usize], target_powers: &[usize]) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag() {
        let source_powers = vec![1, 3, 11, 18, 45, 225];
        let target_degree = 1305;
        let ps_low_deg = 44;
        let mut target_powers = vec![];
        for i in 1..(ps_low_deg + 1) {
            target_powers.push(i);
        }

        let mut high_degree_start = ps_low_deg + 1;
        let high_degree_end = (target_degree / high_degree_start) * high_degree_start;
        while high_degree_start <= high_degree_end {
            target_powers.push(high_degree_start);
            high_degree_start += ps_low_deg + 1;
        }
        construct_dag(&source_powers, &target_powers);
    }
}
