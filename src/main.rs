use {
    crate::{
        addresses::Addresses,
        args::{Args, Endian, Size},
        base::Base,
        strings::Strings,
    },
    clap::Parser,
    memmap2::Mmap,
    std::{fs::File, slice::from_raw_parts, time::Instant},
};

mod addresses;
mod args;
mod base;
mod progress;
mod strings;

const PAGE_OFFSET_MASK: usize = 0xFFF;

fn main() {
    let args = Args::parse();
    println!("{:}", args);

    let file = File::open(&args.filename).unwrap();
    let map = unsafe { Mmap::map(&file).unwrap() };
    let bytes = unsafe { from_raw_parts(map.as_ptr(), map.len()) };

    let start = Instant::now();

    match args.size() {
        Size::Bits32 => {
            let strings = Strings::new(&args, bytes);
            let addresses = Addresses::new(
                bytes,
                match args.endian() {
                    Endian::Little => |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => |bytes: &[u8]| u32::from_be_bytes(bytes.try_into().unwrap()),
                },
            );
            let base = Base::new(&strings, &addresses);
            println!("Found base: {}", base);
        }
        Size::Bits64 => {
            let strings = Strings::new(&args, bytes);
            let addresses = Addresses::new(
                bytes,
                match args.endian() {
                    Endian::Little => |bytes: &[u8]| u64::from_le_bytes(bytes.try_into().unwrap()),
                    Endian::Big => |bytes: &[u8]| u64::from_be_bytes(bytes.try_into().unwrap()),
                },
            );
            let base = Base::new(&strings, &addresses);
            println!("Found base: {:}", base);
        }
    };
    let end = start.elapsed();
    println!("Took: {:?}", end);
}
