use {
    crate::{progress::Progress, PAGE_OFFSET_MASK},
    dashmap::DashMap,
    indicatif::ParallelProgressIterator,
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    std::{hash::Hash, mem::size_of, num::TryFromIntError, ops::BitAnd},
};

pub struct Addresses<T> {
    addresses: DashMap<T, Vec<T>>,
}

impl<
        T: Copy
            + Send
            + Sync
            + Default
            + PartialEq
            + Eq
            + Hash
            + BitAnd<Output = T>
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

    fn get_unique_addresses_by_page_offset(frequencies: DashMap<T, usize>) -> DashMap<T, Vec<T>> {
        let pb = Progress::get("Finding unique addresses", frequencies.len());
        let map = DashMap::<T, Vec<T>>::new();
        frequencies
            .par_iter()
            .progress_with(pb)
            .filter(|r| *r.value() != 1)
            .for_each(|r| {
                let &k = r.key();
                let offset = k & T::try_from(PAGE_OFFSET_MASK).unwrap();
                if let Some(mut v) = map.get_mut(&offset) {
                    v.push(k);
                } else {
                    map.insert(k, vec![k]);
                }
            });
        map
    }

    pub fn new<F: Fn(&[u8]) -> T + Sync + Send + Copy>(bytes: &[u8], convert: F) -> Self {
        let frequencies = Self::get_address_frequencies(bytes, convert);
        println!("Found: {:?} addresses", frequencies.len());

        let addresses = Self::get_unique_addresses_by_page_offset(frequencies);
        println!("Found: {:?} unique addresses", addresses.len());
        Self { addresses }
    }

    pub fn get_addresses(&self) -> &DashMap<T, Vec<T>> {
        return &self.addresses;
    }
}
