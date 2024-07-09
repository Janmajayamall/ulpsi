use std::thread::panicking;

use bfv::Modulus;

use crate::time_it;

/// Multiplies a polynomial with a monomial and returns the product.
///
/// Assume monomial is (x - a)
/// and p(x) = [c_0, c_1, ..., c_n] with c_0 as constant
/// then p'(x) = p(x) (x - a) equals
/// p'(x) = xp(x) - ap(x)
/// = [0, c_0, ..., c_{n-1}, c_n] - [ac_0, a_c1, ..., ac_n, 0]
fn poly_mul_monomial(poly: &mut Vec<u32>, a: u32, modq: &Modulus) {
    // make room for another degree
    poly.push(0);

    let degree = poly.len() - 1;

    for i in (1..(degree + 1)).rev() {
        // In p'(x) i_th element is p[i-1] - a*p[i] since x*p(x) increases exponent of each
        // element in p(x) by 1
        poly[i] = modq.sub_mod_fast(
            poly[i - 1] as u64,
            modq.mul_mod_fast(a as u64, poly[i] as u64),
        ) as u32
    }

    // process constant separately as -ac_0
    poly[0] = modq.neg_mod_fast(modq.mul_mod_fast(a as u64, poly[0] as u64)) as u32
}

fn divided_matrix(x: &[u32], y: &[u32], modq: &Modulus) -> Vec<Vec<u32>> {
    let degree = x.len() - 1;

    // construct divided difference matrix
    let mut ddiff = Vec::with_capacity(degree + 1);
    // We don't need an exact matrix since only upper triangle will hold values
    for i in (1..(degree + 1 + 1)).rev() {
        ddiff.push(Vec::with_capacity(i));
    }

    // process 0^th column
    for row in 0..degree + 1 {
        ddiff[row].push(y[row]);
    }

    for col in 1..(degree + 1) {
        for row in 0..((degree + 1) - col) {
            // y[k,...,a] in col_{i-1}
            let y1 = ddiff[row + 1][col - 1] as u64;
            // y[k-1,...,a,b] in col_{i-1}
            let y0 = ddiff[row][col - 1] as u64;

            let y1_y0 = modq.sub_mod_fast(y1, y0);

            let x_1_x0 = modq.sub_mod_fast(x[row + col] as u64, x[row] as u64);
            if x_1_x0 == 0 {
                panic!("Repeated x values with different y values");
            }
            let x1_x0_inv = modq.inv(x_1_x0);

            // (y[k,...,a] - y[k-1,...,b])/(x_k - x_b)
            let v = modq.mul_mod_fast(y1_y0, x1_x0_inv) as u32;

            ddiff[row].push(v);
        }
    }
    ddiff
}

pub fn newton_interpolate(x: &[u32], y: &[u32], modq: u32) -> Vec<u32> {
    if x.len() == 0 {
        return vec![];
    }

    let modq = Modulus::new(modq as u64);

    assert!(x.len() == y.len());
    let divided_matrix = divided_matrix(x, y, &modq);

    let degree = x.len() - 1;

    // apply horner's rule to construct coefficients
    let mut coefficients = vec![0u32];
    for i in (1..(degree + 1)).rev() {
        let a_i = divided_matrix[0][i];
        coefficients[0] = modq.add_mod_fast(coefficients[0] as u64, a_i as u64) as u32;

        // (c_i(x^i) + ... + a_i) * (x - x_{i-1})
        poly_mul_monomial(&mut coefficients, x[i - 1], &modq);
    }

    // handle a_0
    coefficients[0] = modq.add_mod_fast(coefficients[0] as u64, divided_matrix[0][0] as u64) as u32;

    coefficients
}

pub fn evaluate_poly(x: u32, coeffs: &[u32], modq: u32) -> u32 {
    let modq = Modulus::new(modq as u64);
    let mut y = 0;
    let mut x_power = 1;
    coeffs.iter().for_each(|c| {
        y = modq.add_mod_fast(y as u64, modq.mul_mod_fast(*c as u64, x_power as u64)) as u32;
        x_power = modq.mul_mod_fast(x_power as u64, x as u64);
    });
    y
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use rand::{distributions::Uniform, thread_rng, Rng};

    use super::*;

    #[test]
    fn divided_difference_matrix_correct() {
        let x = vec![1, 2, 3, 4, 5, 6];
        let y: Vec<u32> = vec![1, 4, 2, 4, 1, 4];
        let matrix = divided_matrix(&x, &y, &Modulus::new(65537));
        println!("{:?}", matrix);
    }

    #[test]
    fn poly_mul_monomial_works() {
        let mut x = vec![1, 4, 2, 4, 2, 4, 56, 6];
        let modq = Modulus::new(65537);

        poly_mul_monomial(&mut x, 3, &modq);

        dbg!(x);
    }

    #[test]
    fn newton_interpolate_works() {
        let mut rng = thread_rng();
        let degree = 1300;
        let modq = 65537;

        let mut x = vec![];
        let mut y: Vec<u32> = vec![];

        while x.len() != degree {
            let tmp_x = rng.gen::<u32>() % modq;
            if !x.contains(&tmp_x) {
                x.push(tmp_x);
                y.push(rng.gen::<u32>() % modq);
            }
        }

        for _ in 0..100 {
            time_it!("Newton Interpolate", let coeffs = newton_interpolate(&x, &y, modq););

            for i in 0..degree {
                let y_res = evaluate_poly(x[i], &coeffs, modq);
                assert_eq!(y_res, y[i]);
            }
        }
    }

    #[test]
    fn exp() {
        let modq = Modulus::new(65537);
        let count = 10000000;
        let x = thread_rng()
            .sample_iter(Uniform::new(0, 65537))
            .take(count)
            .collect_vec();
        for i in 0..count {
            modq.inv(x[i]);
        }
    }
}
