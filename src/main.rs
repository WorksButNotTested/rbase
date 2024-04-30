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
        mem::size_of,
        num::TryFromIntError,
        ops::{BitAnd, Sub},
        slice::from_raw_parts,
        thread,
        time::Instant,
    },
};

const PAGE_OFFSET_MASK: usize = 0xFFF;

enum Size {
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

enum Endian {
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
struct Args {
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
    pub max_string_length: usize,

    #[arg(long = "min", help = "Minimum string length", default_value = "10")]
    pub min_string_length: usize,

    #[arg(
        short = 's',
        long = "max-strings",
        help = "Maximum number of strings to sample",
        default_value = "100000"
    )]
    pub max_strings: usize,

    #[arg(
        short = 'a',
        long = "max-addresses",
        help = "Maximum number of addresses to sample",
        default_value = "1000000"
    )]
    pub max_addresses: usize,
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
        writeln!(f, "ARGS")?;
        writeln!(f, "\tfile: {}", self.filename)?;
        writeln!(f, "\tsize: {:}", self.size())?;
        writeln!(f, "\tendian: {:}", self.endian())?;
        writeln!(f, "\tmax: {}", self.max_string_length)?;
        writeln!(f, "\tmin: {}", self.min_string_length)?;
        writeln!(f, "\tmax strings: {}", self.max_strings)?;
        writeln!(f, "\tmax addresses: {}", self.max_addresses)?;
        Ok(())
    }
}

/* Progress */
fn get_progress_bar(msg: &'static str, length: usize) -> indicatif::ProgressBar {
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

trait RBaseTraits<T, const N: usize>:
    Copy
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
    + TryFrom<usize, Error = TryFromIntError>
{
}

impl RBaseTraits<u32, { size_of::<u32>() }> for u32 {}
impl RBaseTraits<u64, { size_of::<u64>() }> for u64 {}

fn get_strings_by_page_offset<T: RBaseTraits<T, N>, const N: usize>(
    bytes: &[u8],
    min_string_length: usize,
    max_string_length: usize,
    max_strings: usize,
) -> DashMap<T, Vec<T>> {
    /* Split the input into a number chunks which overlap by the maximum string length - 1 */
    let chunk_size = bytes.len() / thread::available_parallelism().unwrap();
    let limit = bytes.len();
    let chunks: Vec<(usize, &[u8])> = (0..limit)
        .step_by(chunk_size)
        .map(|chunk_offset| {
            (
                chunk_offset,
                &bytes
                    [chunk_offset..(chunk_offset + chunk_size + max_string_length - 1).min(limit)],
            )
        })
        .collect();

    /* Search each chunk for strings and collect them in a hash set */
    let regex = format!(
        "([[:print:][:space:]]{{{},{}}})\0",
        min_string_length, max_string_length
    );
    let re = Regex::new(&regex).unwrap();
    let offsets = DashSet::<T>::new();
    let progress_bar = get_progress_bar("Finding strings", chunks.len());
    chunks
        .into_par_iter()
        .progress_with(progress_bar)
        .for_each(|(chunk_offset, chunk)| {
            re.find_iter(chunk).for_each(|m| {
                let file_offset = T::try_from(chunk_offset + m.start()).unwrap();
                offsets.insert(file_offset);
            });
        });
    println!("Found: {:?} strings", offsets.len());

    /* Index each string by its page offset */
    let index = DashMap::<T, Vec<T>>::new();
    let progress_bar = get_progress_bar("Indexing strings", offsets.len());
    let page_offset_mask = T::try_from(PAGE_OFFSET_MASK).unwrap();
    offsets
        .into_par_iter()
        .take_any(max_strings)
        .progress_with(progress_bar)
        .for_each(|file_offset| {
            let page_offset = file_offset & page_offset_mask;
            if let Some(mut file_offsets) = index.get_mut(&page_offset) {
                file_offsets.push(file_offset);
            } else {
                index.insert(page_offset, vec![file_offset]);
            }
        });
    index
}

fn get_addresses_by_page_offset<T: RBaseTraits<T, N>, const N: usize>(
    bytes: &[u8],
    read_address_bytes: fn([u8; N]) -> T,
    max_addresses: usize,
) -> DashMap<T, Vec<T>> {
    let chunks = bytes
        .chunks(size_of::<T>())
        .map(|c| c.try_into().unwrap())
        .collect::<Vec<[u8; N]>>();

    /* Search each chunk for addresses and collect them in a hash set */
    let progress_bar = get_progress_bar("Finding addresses", chunks.len());
    let addresses = DashSet::<T>::new();
    chunks
        .into_par_iter()
        .progress_with(progress_bar)
        .map(|bytes| read_address_bytes(bytes))
        .filter(|&address| address != T::default())
        .for_each(|address| {
            addresses.insert(address);
        });
    println!("Found: {:?} addresses", addresses.len());

    /* Index each address by its page offset */
    let index = DashMap::<T, Vec<T>>::new();
    let progress_bar = get_progress_bar("Indexing addresses", addresses.len());
    let page_offset_mask = T::try_from(PAGE_OFFSET_MASK).unwrap();
    addresses
        .into_par_iter()
        .take_any(max_addresses)
        .progress_with(progress_bar)
        .for_each(|address| {
            let page_offset = address & page_offset_mask;
            if let Some(mut v) = index.get_mut(&page_offset) {
                v.push(address);
            } else {
                index.insert(page_offset, vec![address]);
            }
        });
    index
}

fn get_base_address<T: RBaseTraits<T, N>, const N: usize>(
    args: &Args,
    bytes: &[u8],
    read_address_bytes: fn([u8; N]) -> T,
) -> Option<T> {
    let strings_index = get_strings_by_page_offset(
        bytes,
        args.min_string_length,
        args.max_string_length,
        args.max_strings,
    );
    let addresses_index =
        get_addresses_by_page_offset(bytes, read_address_bytes, args.max_addresses);

    /* Subtract the string offsets from the addresses to determine candidate base addresses.
    Update a hashtable with the frequency of each candidate base address.*/
    let progress_bar = get_progress_bar("Collecting candidate base addresses", strings_index.len());
    let base_addresses = DashMap::<T, usize>::new();
    strings_index
        .into_par_iter()
        .progress_with(progress_bar)
        .for_each(|(string_page_offset, string_file_offsets)| {
            if let Some(addresses) = addresses_index.get(&string_page_offset) {
                for &string_file_offset in string_file_offsets.iter() {
                    for &address in addresses
                        .iter()
                        .filter(|&&address| address >= string_file_offset)
                    {
                        *base_addresses
                            .entry(address - string_file_offset)
                            .or_insert(0) += 1;
                    }
                }
            }
        });

    let num_candidates = base_addresses.len();
    println!("Found: {:?} candidate base addresses", num_candidates);

    /* Filter out any candidates which don't appear more than once */
    let recurring: DashMap<T, usize> = base_addresses
        .into_par_iter()
        .filter(|&(_k, v)| v > 1)
        .collect();
    println!(
        "Found: {:?} recurring candidate base addresses",
        recurring.len()
    );

    /* Sort the recurring candidates by frequency */
    let mut sorted: Vec<(T, usize)> = recurring.into_iter().collect();
    sorted.sort_by(|(_a1, v1), (_a2, v2)| v2.cmp(v1));

    /* Print the top 10 candidates */
    for (idx, (base, frequency)) in sorted.iter().take(10).enumerate() {
        let pct = 100.0 * (*frequency as f64) / (num_candidates as f64);
        println!(
            "{:2}: 0x{base:0width$x}: {frequency} ({pct:.2}%)",
            idx + 1,
            width = N * 2
        );
    }

    /* Return the most frequent candidate base address */
    let (base, _frequency) = sorted.first().cloned()?;
    Some(base)
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
            if let Some(base) = get_base_address(
                &args,
                bytes,
                match args.endian() {
                    Endian::Little => u32::from_le_bytes,
                    Endian::Big => u32::from_be_bytes,
                },
            ) {
                println!("Found base: {:0x}", base);
            } else {
                println!("No base found");
            }
        }
        Size::Bits64 => {
            if let Some(base) = get_base_address(
                &args,
                bytes,
                match args.endian() {
                    Endian::Little => u64::from_le_bytes,
                    Endian::Big => u64::from_be_bytes,
                },
            ) {
                println!("Found base: {:x}", base);
            } else {
                println!("No base found");
            }
        }
    };
    let end = start.elapsed();
    println!("Took: {:?}", end);
}
