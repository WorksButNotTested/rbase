use {
    clap::Parser,
    std::fmt::{Display, Formatter, Result},
};

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
