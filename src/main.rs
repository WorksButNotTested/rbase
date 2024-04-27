use {
    clap::Parser,
    dashmap::{DashMap, DashSet},
    indicatif::{ParallelProgressIterator, ProgressBar, ProgressFinish, ProgressStyle},
    memmap2::Mmap,
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    regex::bytes::Regex,
    std::{
        fmt::{Display, Formatter, LowerHex, Result},
        fs::File,
        hash::Hash,
        marker::PhantomData,
        mem::size_of,
        num::TryFromIntError,
        ops::{BitAnd, Sub},
        slice::from_raw_parts,
        thread,
        time::Instant,
    },
};

const PAGE_OFFSET_MASK: usize = 0xFFF;

pub enum Size {
    Bits32,
    Bits64,
}

impl Display for Size {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Size::Bits32 => write!(f, "32-bit"),
            Size::Bits64 => write!(f, "64-bit"),
        }
    }
}

pub enum Endian {
    Little,
    Big,
}

impl Display for Endian {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            Endian::Little => write!(f, "little"),
            Endian::Big => write!(f, "big"),
        }
    }
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(help = "Name of the file to process")]
    pub filename: String,

    #[arg(
        long = "32",
        help = "File is 32-bit (default)",
        conflicts_with = "is_64bit"
    )]
    is_32bit: bool,

    #[arg(long = "64", help = "File is 64-bit", conflicts_with = "is_32bit")]
    is_64bit: bool,

    #[arg(
        long = "little",
        help = "File is little-endian (default)",
        conflicts_with = "is_big_endian"
    )]
    is_little_endian: bool,

    #[arg(
        long = "big",
        help = "File is big-endian",
        conflicts_with = "is_little_endian"
    )]
    is_big_endian: bool,

    #[arg(long = "max", help = "Maximum string length", default_value = "1024")]
    pub max: usize,

    #[arg(long = "min", help = "Minimum string length", default_value = "10")]
    pub min: usize,

    #[arg(
        short = 'j',
        long = "jobs",
        help = "Number of jobs per core",
        default_value = "8"
    )]
    pub jobs: usize,
}

impl Args {
    pub fn size(&self) -> Size {
        if self.is_64bit {
            Size::Bits64
        } else {
            Size::Bits32
        }
    }

    pub fn endian(&self) -> Endian {
        if self.is_big_endian {
            Endian::Big
        } else {
            Endian::Little
        }
    }
}

impl Display for Args {
    fn fmt(&self, f: &mut Formatter) -> Result {
        writeln!(f, "file: {}", self.filename)?;
        writeln!(f, "size: {:}", self.size())?;
        writeln!(f, "endian: {:}", self.endian())?;
        writeln!(f, "max: {}", self.max)?;
        writeln!(f, "min: {}", self.min)?;
        Ok(())
    }
}

pub struct RBase<T> {
    _phantom: PhantomData<T>,
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
    > RBase<T>
{
    /* Progress */
    pub fn get_progress_bar(msg: &'static str, length: usize) -> indicatif::ProgressBar {
        let progress_bar = ProgressBar::new(length as u64)
            .with_message(format!("{msg:<50}"))
            .with_finish(ProgressFinish::AndLeave);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise:.green}] [{eta_precise:.cyan}] {msg:.magenta} ({percent:.bold}%) [{bar:30.cyan/blue}]",
                )
                .unwrap()
                .progress_chars("█░")
        );
        progress_bar
    }

    /* Strings */
    fn get_overlapping_chunks(bytes: &[u8], overlap: usize) -> Vec<(usize, &[u8])> {
        let chunk_size = bytes.len() / thread::available_parallelism().unwrap();
        let limit = bytes.len();
        (0..limit)
            .step_by(chunk_size)
            .map(|i| (i, &bytes[i..(i + chunk_size + overlap).min(limit)]))
            .collect()
    }

    fn get_string_offsets(min: usize, max: usize, chunks: Vec<(usize, &[u8])>) -> DashSet<T> {
        let regex = format!("([a-zA-Z0-9_]{{{},{}}})\0", min, max);
        let re = Regex::new(&regex).unwrap();
        let pb = Self::get_progress_bar("Finding strings", chunks.len());
        let set = DashSet::<T>::new();
        chunks
            .into_par_iter()
            .progress_with(pb)
            .for_each(|(offset, chunk)| {
                re.find_iter(chunk).for_each(|m| {
                    let k = T::try_from(offset + m.start()).unwrap();
                    set.insert(k);
                });
            });
        println!("Found: {:?} strings", set.len());
        set
    }

    fn index_strings_by_page_offset(addresses: DashSet<T>) -> DashMap<T, Vec<T>> {
        let pb = Self::get_progress_bar("Indexing strings", addresses.len());
        let map = DashMap::<T, Vec<T>>::new();
        addresses.into_par_iter().progress_with(pb).for_each(|k| {
            let offset = k & T::try_from(PAGE_OFFSET_MASK).unwrap();
            if let Some(mut v) = map.get_mut(&offset) {
                v.push(k);
            } else {
                map.insert(offset, vec![k]);
            }
        });
        map
    }

    pub fn get_strings_by_page_offset(min: usize, max: usize, bytes: &[u8]) -> DashMap<T, Vec<T>> {
        let chunks = Self::get_overlapping_chunks(bytes, max - 1);
        let addresses = Self::get_string_offsets(min, max, chunks);
        println!("Found: {:?} unique strings", addresses.len());
        let index = Self::index_strings_by_page_offset(addresses);
        index
    }

    /* Addresses */
    fn get_address_frequencies<F: Fn(&[u8]) -> T + Sync + Send>(
        bytes: &[u8],
        convert: F,
    ) -> DashMap<T, usize> {
        let chunks = bytes.chunks(size_of::<T>()).collect::<Vec<&[u8]>>();
        let pb = Self::get_progress_bar("Reading addresses", chunks.len());
        let map = DashMap::<T, usize>::new();
        chunks
            .into_par_iter()
            .progress_with(pb)
            .map(|p| convert(p))
            .filter(|&p| p != T::default())
            .for_each(|ptr| {
                *map.entry(ptr).or_insert(0) += 1;
            });
        map
    }

    fn index_unique_addresses_by_page_offset(frequencies: DashMap<T, usize>) -> DashMap<T, Vec<T>> {
        let map = DashMap::<T, Vec<T>>::new();
        let pb = Self::get_progress_bar("Finding unique addresses", frequencies.len());
        frequencies
            .into_par_iter()
            .progress_with(pb)
            .filter_map(|(k, v)| if v > 1 { Some(k) } else { None })
            .for_each(|k| {
                let offset = k & T::try_from(PAGE_OFFSET_MASK).unwrap();
                if let Some(mut v) = map.get_mut(&offset) {
                    v.push(k);
                } else {
                    map.insert(k, vec![k]);
                }
            });
        map
    }

    pub fn get_addresses_by_page_offset<F: Fn(&[u8]) -> T + Sync + Send + Copy>(
        bytes: &[u8],
        convert: F,
    ) -> DashMap<T, Vec<T>> {
        let frequencies = Self::get_address_frequencies(bytes, convert);
        println!("Found: {:?} addresses", frequencies.len());

        let addresses = Self::index_unique_addresses_by_page_offset(frequencies);
        println!("Found: {:?} unique addresses", addresses.len());
        addresses
    }

    /* Addresses */
    fn get_candidate_base_addresses(
        strings: &DashMap<T, Vec<T>>,
        addresses: &DashMap<T, Vec<T>>,
    ) -> DashMap<T, usize> {
        let pb = Self::get_progress_bar("Collecting candidate base addresses", strings.len());
        let map = DashMap::<T, usize>::new();
        strings.into_par_iter().progress_with(pb).for_each(|r| {
            let (offset, strings) = r.pair();
            if let Some(addresses) = addresses.get(offset) {
                for &s in strings.iter() {
                    for &a in addresses.iter().filter(|&&a| a > s) {
                        *map.entry(a - s).or_insert(0) += 1;
                    }
                }
            }
        });
        map
    }

    fn remove_unique_base_addresses(bases: DashMap<T, usize>) -> DashMap<T, usize> {
        bases.into_par_iter().filter(|&(_k, v)| v > 1).collect()
    }

    fn sort_candidate_base_addresses_by_frequency(collated: DashMap<T, usize>) -> Vec<(T, usize)> {
        let mut sorted: Vec<(T, usize)> = collated.into_iter().collect();
        sorted.sort_by(|(_a1, v1), (_a2, v2)| v2.cmp(v1));
        sorted
    }

    pub fn get_most_frequent_candidate_base_address(
        strings: &DashMap<T, Vec<T>>,
        addresses: &DashMap<T, Vec<T>>,
    ) -> T {
        let base_addresses = Self::get_candidate_base_addresses(strings, addresses);
        let num_candidates = base_addresses.len();
        println!("Found: {:?} candidates", num_candidates);

        let filtered = Self::remove_unique_base_addresses(base_addresses);
        println!("Found: {:?} filtered candidates", filtered.len());

        let sorted = Self::sort_candidate_base_addresses_by_frequency(filtered);
        for (idx, (base, frequency)) in sorted.iter().take(10).enumerate() {
            let pct = 100.0 * (*frequency as f64) / (num_candidates as f64);
            println!("{:2}: {base:x}: {frequency} ({pct:.2}%)", idx + 1);
        }

        let (base, _frequency) = sorted.first().unwrap().clone();
        base
    }
}

fn main() {
    let args = Args::parse();
    println!("{:}", args);

    let file = File::open(&args.filename).unwrap();
    let map = unsafe { Mmap::map(&file).unwrap() };
    let bytes = unsafe { from_raw_parts(map.as_ptr(), map.len()) };

    let start = Instant::now();

    match args.size() {
        Size::Bits32 => {
            let strings = RBase::get_strings_by_page_offset(args.min, args.max, bytes);
            let addresses = RBase::get_addresses_by_page_offset(
                bytes,
                match args.endian() {
                    Endian::Little => |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => |bytes: &[u8]| u32::from_be_bytes(bytes.try_into().unwrap()),
                },
            );
            let base = RBase::get_most_frequent_candidate_base_address(&strings, &addresses);
            println!("Found base: {:x}", base);
        }
        Size::Bits64 => {
            let strings = RBase::get_strings_by_page_offset(args.min, args.max, bytes);
            let addresses = RBase::get_addresses_by_page_offset(
                bytes,
                match args.endian() {
                    Endian::Little => |bytes: &[u8]| u64::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => |bytes: &[u8]| u64::from_be_bytes(bytes.try_into().unwrap()),
                },
            );
            let base = RBase::get_most_frequent_candidate_base_address(&strings, &addresses);
            println!("Found base: {:x}", base);
        }
    };
    let end = start.elapsed();
    println!("Took: {:?}", end);
}
