use {
    crate::{args::Args, progress::Progress, PAGE_OFFSET_MASK},
    dashmap::{DashMap, DashSet},
    indicatif::ParallelProgressIterator,
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    regex::bytes::Regex,
    std::{hash::Hash, num::TryFromIntError, ops::BitAnd, thread},
};

pub struct Strings<T> {
    index: DashMap<T, Vec<T>>,
}

impl<
        T: Copy
            + Send
            + Sync
            + Hash
            + Eq
            + BitAnd<Output = T>
            + TryFrom<usize, Error = TryFromIntError>,
    > Strings<T>
{
    fn get_overlapping_chunks(bytes: &[u8], overlap: usize) -> Vec<(usize, &[u8])> {
        let chunk_size = bytes.len() / thread::available_parallelism().unwrap();
        let limit = bytes.len();
        let mut chunks = Vec::new();
        for i in (0..limit).step_by(chunk_size) {
            let end = i + chunk_size + overlap;
            if end >= limit {
                chunks.push((i, &bytes[i..]));
            } else {
                chunks.push((i, &bytes[i..end]));
            }
        }
        chunks
    }

    fn find_string_addresses_in_chunk(re: &Regex, chunk_offset: usize, bytes: &[u8]) -> Vec<T> {
        let mut strings = Vec::new();
        let mut i = 0_usize;
        while i < bytes.len() {
            let slice = &bytes[i..];
            if let Some(m) = re.find(slice) {
                let string_offset = chunk_offset + i + m.start();
                let address = T::try_from(string_offset).unwrap();
                strings.push(address);
                i += m.end();
            } else {
                break;
            }
        }
        return strings;
    }

    fn get_string_addresses(re: &Regex, chunks: Vec<(usize, &[u8])>) -> DashSet<T> {
        let pb = Progress::get("Finding strings", chunks.len());
        let set = DashSet::<T>::new();
        chunks
            .par_iter()
            .progress_with(pb)
            .for_each(|(offset, chunk)| {
                let addresses = Self::find_string_addresses_in_chunk(&re, *offset, chunk);
                for k in addresses {
                    set.insert(k);
                }
            });
        println!("Found: {:?} strings", set.len());
        set
    }

    fn index_strings_by_page_offset(addresses: DashSet<T>) -> DashMap<T, Vec<T>> {
        let pb = Progress::get("Indexing strings", addresses.len());
        let map = DashMap::<T, Vec<T>>::new();
        addresses.par_iter().progress_with(pb).for_each(|r| {
            let &k = r.key();
            let offset = k & T::try_from(PAGE_OFFSET_MASK).unwrap();
            if let Some(mut v) = map.get_mut(&offset) {
                v.push(k);
            } else {
                map.insert(offset, vec![k]);
            }
        });
        map
    }

    pub fn new(args: &Args, bytes: &[u8]) -> Self {
        let chunks = Self::get_overlapping_chunks(bytes, args.max - 1);
        let regex = format!("([a-zA-Z0-9_]{{{},{}}})\0", args.min, args.max);
        let re = Regex::new(&regex).unwrap();
        let addresses = Self::get_string_addresses(&re, chunks);
        println!("Found: {:?} unique strings", addresses.len());
        let index = Strings::index_strings_by_page_offset(addresses);
        return Self { index };
    }

    pub fn get(&self) -> &DashMap<T, Vec<T>> {
        return &self.index;
    }
}
