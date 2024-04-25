use {
    crate::progress::Progress,
    indicatif::{ParallelProgressIterator, ProgressIterator},
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    std::{collections::HashMap, hash::Hash, mem::size_of},
};

pub struct Addresses<T> {
    addresses: Vec<T>,
}

impl<T: Copy + Send + Sync + Default + PartialEq + Eq + Hash> Addresses<T> {
    fn read_addresses<F: Fn(&[u8]) -> T + Sync + Send>(bytes: &[u8], convert: F) -> Vec<T> {
        let chunks = bytes.chunks(size_of::<T>()).collect::<Vec<&[u8]>>();
        let pb = Progress::get("Reading addresses", chunks.len());
        chunks
            .par_iter()
            .progress_with(pb)
            .map(|&p| convert(p))
            .filter(|&p| p != T::default())
            .collect::<Vec<T>>()
    }

    fn get_freqencies(addresses: Vec<T>) -> Vec<HashMap<T, usize>> {
        /* Calculate frequencies in parallel */
        let pb = Progress::get("Calculating frequencies", addresses.len());
        addresses
            .par_iter()
            .progress_with(pb)
            .fold(HashMap::<T, usize>::new, |mut map, ptr| {
                if let Some(v) = map.get(ptr) {
                    map.insert(*ptr, v + 1);
                } else {
                    map.insert(*ptr, 1);
                }
                map
            })
            .collect::<Vec<HashMap<T, usize>>>()
    }

    fn collate_frequencies(frequencies: Vec<HashMap<T, usize>>) -> HashMap<T, usize> {
        let pb = Progress::get("Collating frequencies", frequencies.len());
        frequencies.into_iter().progress_with(pb).fold(
            HashMap::<T, usize>::new(),
            |mut map, chunk| {
                for (k, v) in chunk {
                    if let Some(v) = map.get(&k) {
                        map.insert(k, v + 1);
                    } else {
                        map.insert(k, v);
                    }
                }
                map
            },
        )
    }

    fn get_unique_addresses(frequencies: HashMap<T, usize>) -> Vec<T> {
        let pb = Progress::get("Finding unique addresses", frequencies.len());
        frequencies
            .par_iter()
            .progress_with(pb)
            .filter_map(|(k, v)| if *v == 1 { Some(*k) } else { None })
            .collect()
    }

    pub fn new<F: Fn(&[u8]) -> T + Sync + Send + Copy>(bytes: &[u8], convert: F) -> Self {
        let addresses = Self::read_addresses(bytes, convert);
        println!("Found: {:?} addresses", addresses.len());
        let frequencies = Self::get_freqencies(addresses);
        let collated = Self::collate_frequencies(frequencies);
        let unique = Self::get_unique_addresses(collated);
        println!("Found: {:?} unique addresses", unique.len());
        Self { addresses: unique }
    }

    pub fn get_addresses(&self) -> &Vec<T> {
        return &self.addresses;
    }
}
