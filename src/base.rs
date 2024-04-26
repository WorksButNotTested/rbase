use {
    crate::{progress::Progress, Addresses, Strings, PAGE_OFFSET_MASK},
    dashmap::DashMap,
    indicatif::ParallelProgressIterator,
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    std::{
        fmt::{Display, Formatter, LowerHex, Result},
        hash::Hash,
        num::TryFromIntError,
        ops::{BitAnd, Deref, Sub},
    },
};

pub struct Base<T> {
    base: T,
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
            + Sub<Output = T>
            + PartialOrd
            + LowerHex
            + TryFrom<usize, Error = TryFromIntError>,
    > Base<T>
{
    fn get_base_addresses(strings: &Strings<T>, addresses: &Addresses<T>) -> DashMap<T, usize> {
        let list = addresses.get_addresses();
        let pb = Progress::get("Collecting candidate base addresses", list.len());
        let map = DashMap::<T, usize>::new();
        list.par_iter().progress_with(pb).for_each(|&ptr| {
            let offset = ptr & T::try_from(PAGE_OFFSET_MASK).unwrap();
            if let Some(strings) = strings.get().get(&offset) {
                for &s in strings.deref() {
                    if ptr > s {
                        let base = ptr - s;
                        *map.entry(base).or_insert(0) += 1;
                    }
                }
            }
        });
        map
    }

    fn filter_base_addresses(bases: DashMap<T, usize>) -> DashMap<T, usize> {
        let map = DashMap::<T, usize>::new();
        let pb = Progress::get("Filtering base addresses", bases.len());
        bases.par_iter().progress_with(pb).for_each(|r| {
            let (&k, &v) = r.pair();
            if v > 1 {
                map.insert(k, v);
            }
        });
        map
    }

    fn get_ordered_base_addresses(collated: DashMap<T, usize>) -> Vec<(T, usize)> {
        let mut sorted: Vec<(T, usize)> = collated.into_iter().collect();
        sorted.sort_by(|(_a1, v1), (_a2, v2)| v2.cmp(v1));
        sorted
    }

    pub fn new(strings: &Strings<T>, addresses: &Addresses<T>) -> Self {
        let base_addresses = Self::get_base_addresses(strings, addresses);
        let num_candidates = base_addresses.len();
        println!("Found: {:?} candidates", num_candidates);

        let filtered = Self::filter_base_addresses(base_addresses);
        let num_filtered = filtered.len();
        println!("Found: {:?} filtered candidates", num_filtered);

        let sorted = Self::get_ordered_base_addresses(filtered);
        for (idx, (base, frequency)) in sorted.iter().take(10).enumerate() {
            let pct = 100.0 * (*frequency as f64) / (num_candidates as f64);
            println!("{:2}: {base:x}: {frequency} ({pct:.2}%)", idx + 1);
        }

        let (base, _frequency) = sorted.first().unwrap().clone();
        Self { base }
    }
}

impl<T: LowerHex> Display for Base<T> {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{:#x}", self.base)
    }
}
