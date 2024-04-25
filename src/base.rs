use {
    crate::{progress::Progress, Addresses, Strings, PAGE_OFFSET_MASK},
    indicatif::{ParallelProgressIterator, ProgressIterator},
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    std::{
        collections::HashMap,
        fmt::{Display, Formatter, LowerHex, Result},
        hash::Hash,
        num::TryFromIntError,
        ops::{BitAnd, Sub},
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
    fn get_base_addresses(strings: &Strings<T>, addresses: &Addresses<T>) -> Vec<T> {
        let list = addresses.get_addresses();
        let pb = Progress::get("Collecting candidate base addresses", list.len());
        list.par_iter()
            .progress_with(pb)
            .map(|&ptr| {
                let mut result = Vec::<T>::new();
                let offset = ptr & T::try_from(PAGE_OFFSET_MASK).unwrap();
                if let Some(strings) = strings.get(&offset) {
                    for &s in strings {
                        if ptr > s {
                            let base = ptr - s;
                            result.push(base);
                        }
                    }
                }
                result
            })
            .flatten()
            .collect::<Vec<T>>()
    }

    fn get_candidate_base_address_frequencies(bases: Vec<T>) -> Vec<HashMap<T, usize>> {
        let pb = Progress::get("Collecting candidate base address frequencies", bases.len());
        bases
            .par_iter()
            .progress_with(pb)
            .fold(HashMap::<T, usize>::new, |mut map, &base| {
                if let Some(v) = map.get(&base) {
                    map.insert(base, *v + 1);
                } else {
                    map.insert(base, 1);
                }
                map
            })
            .collect::<Vec<HashMap<T, usize>>>()
    }

    fn collate_candidate_base_address_frequencies(
        bases: Vec<HashMap<T, usize>>,
    ) -> HashMap<T, usize> {
        let pb = Progress::get("Collating base address candidate frequencies", bases.len());
        bases.into_iter().progress_with(pb).fold(
            HashMap::<T, usize>::new(),
            |mut map, base_count| {
                for (base, count) in base_count.into_iter() {
                    if let Some(v) = map.get(&base) {
                        map.insert(base, *v + count);
                    } else {
                        map.insert(base, count);
                    }
                }
                map
            },
        )
    }

    fn get_ordered_base_addresses(collated: HashMap<T, usize>) -> Vec<(T, usize)> {
        let mut sorted: Vec<(T, usize)> = collated.into_iter().collect();
        sorted.sort_by(|(_a1, v1), (_a2, v2)| v2.cmp(v1));
        sorted
    }

    pub fn new(strings: &Strings<T>, addresses: &Addresses<T>) -> Self {
        let base_addresses = Self::get_base_addresses(strings, addresses);
        let num_candidates = base_addresses.len();
        println!("Found: {:?} candidates", num_candidates);

        let frequencies = Self::get_candidate_base_address_frequencies(base_addresses);
        let collated = Self::collate_candidate_base_address_frequencies(frequencies);
        println!("Found: {:?} unique candidates", collated.len());

        let sorted = Self::get_ordered_base_addresses(collated);
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
