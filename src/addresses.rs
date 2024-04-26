use {
    crate::progress::Progress,
    dashmap::DashMap,
    indicatif::ParallelProgressIterator,
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    std::{hash::Hash, mem::size_of, num::TryFromIntError},
};

pub struct Addresses<T> {
    addresses: Vec<T>,
}

impl<
        T: Copy
            + Send
            + Sync
            + Default
            + PartialEq
            + Eq
            + Hash
            + TryFrom<usize, Error = TryFromIntError>,
    > Addresses<T>
{
    fn get_address_frequencies<F: Fn(&[u8]) -> T + Sync + Send>(
        bytes: &[u8],
        convert: F,
    ) -> DashMap<T, usize> {
        let chunks = bytes.chunks(size_of::<T>()).collect::<Vec<&[u8]>>();
        let pb = Progress::get("Reading addresses", chunks.len());
        let map = DashMap::<T, usize>::new();
        chunks
            .par_iter()
            .progress_with(pb)
            .map(|&p| convert(p))
            .filter(|&p| p != T::default())
            .for_each(|ptr| {
                *map.entry(ptr).or_insert(0) += 1;
            });
        map
    }

    fn get_unique_addresses(frequencies: DashMap<T, usize>) -> Vec<T> {
        let pb = Progress::get("Finding unique addresses", frequencies.len());
        frequencies
            .par_iter()
            .progress_with(pb)
            .filter_map(|r| {
                let (&k, &v) = r.pair();
                if v == 1 {
                    Some(k)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn new<F: Fn(&[u8]) -> T + Sync + Send + Copy>(bytes: &[u8], convert: F) -> Self {
        let frequencies = Self::get_address_frequencies(bytes, convert);
        println!("Found: {:?} addresses", frequencies.len());
        let unique = Self::get_unique_addresses(frequencies);
        println!("Found: {:?} unique addresses", unique.len());
        Self { addresses: unique }
    }

    pub fn get_addresses(&self) -> &Vec<T> {
        return &self.addresses;
    }
}
