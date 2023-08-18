use itertools::Itertools;
use ring::digest::{self, Digest};
use std::collections::HashMap;

fn sha256(item: u128) -> Digest {
    digest::digest(&digest::SHA256, &item.to_le_bytes())
}

pub struct Cuckoo {
    no_of_tables: u8,
    table_size: u32,
}
impl Cuckoo {
    pub fn new(no_of_tables: u8, table_size: u32) -> Cuckoo {
        // Cannot allow greater than 8 hash tables since the way hashing is implementated limits to 8 hash outputs at max.
        assert!(no_of_tables <= 8);
        Cuckoo {
            no_of_tables,
            table_size,
        }
    }

    /// Hashes the data and return indices in each hash table
    pub fn table_indices(&self, data: u128) -> Vec<u32> {
        let digest = sha256(data);

        // We divide the digest in chunks of 32 bits and view each chunk as ouput from different hash functions
        let outputs = digest
            .as_ref()
            .chunks_exact(4)
            .take(self.no_of_tables as usize)
            .map(|o| {
                let mut output = 0u32;
                o.iter()
                    .enumerate()
                    .for_each(|(i, b)| output += (*b as u32) * (1 << (i * 8)));
                output % self.table_size
            })
            .collect_vec();

        outputs
    }
}

#[derive(Clone, Debug)]
pub struct HashTableEntry(u128, u8);
impl HashTableEntry {
    pub fn entry_value(&self) -> u128 {
        self.0
    }

    pub fn hash_index(&self) -> usize {
        self.1 as usize
    }

    pub fn increase_hash_index(&mut self) {
        self.1 += 1;
    }
}

pub fn construct_hash_tables(input: &[HashTableEntry], cuckoo: &Cuckoo) {
    let mut hash_tables = vec![HashMap::new(); cuckoo.no_of_tables as usize];

    let mut curr_index = 0;
    let mut curr_element = Some(input[curr_index].clone());

    let mut stack = vec![];

    while curr_index < input.len() {
        if curr_element.is_none() {
            curr_element = Some(input[curr_index].clone());
        }

        let data = curr_element.clone().unwrap();
        let indices = cuckoo.table_indices(data.entry_value());

        let old_value = hash_tables[data.hash_index()].insert(indices[data.hash_index()], data);

        if old_value.is_some() {
            let mut v = old_value.unwrap();
            v.increase_hash_index();

            if v.hash_index() == cuckoo.no_of_tables as usize {
                stack.push(v);
                curr_index += 1;
                curr_element = None;
            } else {
                curr_element = Some(v);
            }
        } else {
            curr_index += 1;
            curr_element = None;
        }
    }
    dbg!(stack.len());
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use rand::{thread_rng, Rng};

    use super::*;

    #[test]
    fn hash_data_works() {
        let mut rng = thread_rng();

        let no_of_hash_tables = 3;
        let table_size = 4096;

        let hasher = Cuckoo::new(no_of_hash_tables as u8, table_size);

        // let indices = hasher.table_indices(rng.gen());

        let mut queue = vec![];
        for i in 0..3500 {
            let data: u128 = rng.gen();
            queue.push(HashTableEntry(data, 0));
        }

        construct_hash_tables(&queue, &hasher);
    }
}
