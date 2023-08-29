use crate::PsiParams;

use super::{EvalPolyDegree, InnerBox};
use bfv::{Ciphertext, Encoding, EvaluationKey, Evaluator, Plaintext, Representation};
use itertools::{izip, Itertools};
use ndarray::Array2;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
};
use traits::TryEncodingWithParameters;

#[derive(Clone, Debug)]
pub struct PSParams {
    low_degree: usize,
    total_degree: usize,
    powers: Vec<usize>,
}

impl PSParams {
    pub fn new(low_degree: usize, total_degree: usize) -> PSParams {
        let mut high_degree_start = low_degree + 1;
        let high_degree_end = (total_degree / high_degree_start) * high_degree_start;

        let mut powers = (1..(low_degree + 1)).into_iter().map(|i| i).collect_vec();
        while high_degree_start <= high_degree_end {
            powers.push(high_degree_start);
            high_degree_start += low_degree + 1;
        }

        PSParams {
            low_degree,
            total_degree,
            powers,
        }
    }

    pub fn low_degree(&self) -> usize {
        self.low_degree
    }

    pub fn powers(&self) -> &[usize] {
        &self.powers
    }

    pub fn eval_degree(&self) -> EvalPolyDegree {
        EvalPolyDegree(self.total_degree as u32)
    }
}

pub fn ps_evaluate_poly(
    evalutor: &Evaluator,
    ek: &EvaluationKey,
    x_powers: &HashMap<usize, Ciphertext>,
    ps_params: &PSParams,
    coefficients: &Array2<u32>,
    level: usize,
) -> Ciphertext {
    // validate coefficients are well formed for interpolation
    assert_eq!(
        coefficients.shape(),
        [evalutor.params().degree, ps_params.total_degree + 1]
    );

    let high_degree = ps_params.low_degree + 1;
    let inner_loop_count = high_degree;
    let outer_loop_count = ps_params.total_degree / (ps_params.low_degree + 1);
    let mut outer_sum = Ciphertext::placeholder();
    let mut first_inner_sum = Ciphertext::placeholder();

    for m in 0..(outer_loop_count + 1) {
        let mut inner_sum = Ciphertext::placeholder();
        for k in 1..inner_loop_count {
            let degree = m * inner_loop_count + k;

            if degree > ps_params.total_degree {
                break;
            }

            let pt = Plaintext::try_encoding_with_parameters(
                coefficients.column(degree),
                evalutor.params(),
                Encoding::simd(level, bfv::PolyCache::Mul(bfv::PolyType::Q)),
            );

            let op1 = x_powers.get(&k).unwrap();

            if k == 1 {
                inner_sum = evalutor.mul_plaintext(op1, &pt);
            } else {
                evalutor.add_assign(&mut inner_sum, &evalutor.mul_plaintext(op1, &pt));
            }
        }

        // add constant (ie inner degree 0)
        if m * inner_loop_count <= ps_params.total_degree {
            let pt = Plaintext::try_encoding_with_parameters(
                coefficients.column(m * inner_loop_count),
                evalutor.params(),
                Encoding::simd(
                    level,
                    bfv::PolyCache::AddSub(bfv::Representation::Evaluation),
                ),
            );
            evalutor.add_assign_plaintext(&mut inner_sum, &pt);
        }

        if m == 0 {
            first_inner_sum = inner_sum;
            // change representation to Coefficient for adding to rest
            evalutor.ciphertext_change_representation(
                &mut first_inner_sum,
                Representation::Coefficient,
            );
        } else {
            let op1 = x_powers.get(&(m * high_degree)).unwrap();
            if m == 1 {
                // outer sum is still a placeholder
                outer_sum = evalutor.mul_lazy(&inner_sum, op1);
            } else {
                evalutor.add_assign(&mut outer_sum, &evalutor.mul_lazy(&inner_sum, op1));
            }
        }
    }

    let mut outer_sum = evalutor.scale_and_round(&mut outer_sum);
    outer_sum = evalutor.relinearize(&outer_sum, &ek);

    evalutor.add_assign(&mut outer_sum, &first_inner_sum);

    outer_sum
}

#[cfg(test)]
mod tests {
    use bfv::PolyCache;
    use ndarray::{Array, Array2};
    use rand::{thread_rng, Rng};

    use crate::{
        client::calculate_source_powers,
        poly_interpolate::{evaluate_poly, newton_interpolate},
        utils::{bfv_setup_test, calculate_ps_powers_with_dag, construct_dag},
    };

    use super::*;

    #[test]
    fn ps_works() {
        let mut rng = thread_rng();
        let source_powers = vec![1, 3, 11, 18, 45, 225];
        let ps_params = PSParams::new(44, 1304);
        let modq = 65537;

        let (evaluator, sk) = bfv_setup_test();
        let ek = EvaluationKey::new(evaluator.params(), &sk, &[0], &[], &[], &mut rng);

        // Interpolate a polynomial
        let mut rng = thread_rng();
        let data_points_count = ps_params.total_degree + 1;
        let mut x = vec![];
        let mut y: Vec<u32> = vec![];

        while x.len() != data_points_count {
            let tmp_x = rng.gen::<u32>() % modq;
            if !x.contains(&tmp_x) {
                x.push(tmp_x);
                y.push(rng.gen::<u32>() % modq);
            }
        }
        let coeffs = newton_interpolate(&x, &y, modq);

        // turns coefficients into 2D array just like InnerBox
        let mut coefficients_2d = Array2::zeros((evaluator.params().degree, data_points_count));
        coefficients_2d
            .row_mut(0)
            .as_slice_mut()
            .unwrap()
            .copy_from_slice(&coeffs);

        // evalute polynomial at x_input
        let x_input = x[5];

        // Simulate client ecnrypting source powers for PS
        let input_vec = vec![x_input];
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

        // get target powers for PS on server
        let dag = construct_dag(&source_powers, ps_params.powers());
        let target_power_cts = calculate_ps_powers_with_dag(
            &evaluator,
            &ek,
            &input_source_powers_cts,
            &source_powers,
            ps_params.powers(),
            &dag,
            &ps_params,
        );

        // Evaluate it homomorphically
        let evaluated_ct = ps_evaluate_poly(
            &evaluator,
            &ek,
            &target_power_cts,
            &ps_params,
            &coefficients_2d,
            0,
        );

        dbg!(evaluator.measure_noise(&sk, &evaluated_ct));

        // Check the results
        let evaluated_res =
            evaluator.plaintext_decode(&evaluator.decrypt(&sk, &evaluated_ct), Encoding::default());
        let expected_evaluated_res = evaluate_poly(x_input, &coeffs, modq);

        assert_eq!(evaluated_res[0] as u32, expected_evaluated_res);
    }
}
