use bfv::Modulus;
use itertools::izip;

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

#[cfg(test)]
mod tests {
    // fn calculate_source_powers_works()
}
