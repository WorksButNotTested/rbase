# rBase
`rBase` is a utility designed to determine the base address of arbitrary binary images. It uses a generic technique which works on all kinds of binary images, including BIOSes, boot loaders, kernels etc. And is agnostic to the operating system of the image. It does, however, require the user provide it with the bitness (e.g. 32-bit or 64-bit) and endianness of the image to be examined. It is written in Rust and should run on Linux, MacOS and Windows systems (thanks to the use of [`cargo-dist`](https://crates.io/crates/cargo-dist)). It's primary purpose is to allow the user to correctly set the base address of an image when loading it into a disassembler for analysis.

```
$ cargo run --release -- data/Image-arm32le
   Compiling rbase v0.1.9 (/home/jon/git/rbase)
    Finished release [optimized] target(s) in 0.78s
     Running `target/release/rbase data/Image-arm32le`
ARGS
        file: data/Image-arm32le
        size: 32-bit
        endian: little
        max: 1024
        min: 10
        max strings: 100000
        max addresses: 1000000

  [00:00:00] [00:00:00] Finding strings                                    (100%) [██████████████████████████████]                                                                                                                                                                                                                                                                                                Found: 73725 strings
  [00:00:00] [00:00:00] Indexing strings                                   (100%) [██████████████████████████████]                                                                                                                                                                                                                                                                                                
  [00:00:00] [00:00:00] Finding addresses                                  (100%) [██████████████████████████████]                                                                                                                                                                                                                                                                                                Found: 1156144 addresses
  [00:00:00] [00:00:00] Indexing addresses                                 (100%) [██████████████████████████████]                                                                                                                                                                                                                                                                                                
  [00:00:00] [00:00:00] Collecting candidate base addresses                (100%) [██████████████████████████████]                                                                                                                                                                                                                                                                                                Found: 877878 candidate base addresses
Found: 700734 recurring candidate base addresses
 1: 0xc0208000: 33991 (3.87%)
 2: 0xc0207000: 5808 (0.66%)
 3: 0xc0209000: 5726 (0.65%)
 4: 0xc020a000: 5635 (0.64%)
 5: 0xc0204000: 5529 (0.63%)
 6: 0xc0212000: 5514 (0.63%)
 7: 0xc0206000: 5478 (0.62%)
 8: 0xc020b000: 5463 (0.62%)
 9: 0xc0211000: 5435 (0.62%)
10: 0xc020d000: 5383 (0.61%)
Found base: c0208000
Took: 697.464047ms
```

# Design
`rBase` is highly parallelised and can determine the base address of even large binaries in a few seconds (thanks to the use of [`rayon`](https://crates.io/crates/rayon) and [`dashmap`](https://crates.io/crates/dashmap)). Meanwhile, it's progress is displayed using [`indicatif`](https://docs.rs/indicatif/latest/indicatif/). It is designed to use a simple to understand algorithm and thanks to the ready availability of advanced libraries in the form of of cargo crates, is implemented in less than [400 lines of Rust](src/main.rs) (much of which is boiler plate code). This project makes use of [`devcontainers`](https://code.visualstudio.com/docs/devcontainers/containers) in order to provide a readily reproducible build environment, but it should build on any recent version of Rust and doesn't make use of any [unstable](https://doc.rust-lang.org/unstable-book/) Rust features.

# Algorithm
## Principle
The algorithm used to determine the base address is built on the basis of analysing addresses (pointers) and strings within the binary. Strings are used within binaries for many purposes from symbol names to strings displayed to the user. If we can determine the offset of a string within an image, and the value of a pointer which referencing it, then we can calculate the base address of the image as follows:
```
pointer = base + offset
```
Therefore:
```
base = pointer - offset
```
## Finding Strings and Addresses
We can use regular expressions to find strings within our binary with a relatively low error rate. We simply use a `regex` which looks for a series of consecutive printable or space characters followed by a `NUL` character. This can be fine-tuned at the command line, but by default, `rBase` will search for strings between `10` and `1024` characters in length. However, determining what is a pointer and what is just data is much more error prone, we simply must assume that any aligned `word` within our image is a potential pointer. Whilst we could search for patterns used by symbol tables in various image types, this would prevent our solution being universal. Moreover, even if we determine which `words` are indeed pointers, we have no way of telling which string they should point to.

## Frequency Analysis
Each `string` and `pointer` we compare will give us a different potential base address for our image. By comparing every combination of `string` and `pointer`, we can analyse the frequency at which each base address occurs and the one which is found most frequently is likely to be the actual base address. We can reduce the number of `strings` and `pointers` we compare by sampling our data. Indeed by default we limit the number of `strings` to `100,000` and the number of `pointers` to `1,000,000` (this is configurable by command line parameter). However, this gives us a huge problem space:

```
100,000 strings * 1,000,000 pointers = 100,000,000,000 comparisons
```
This is too much work for even the most performant processor even when distributed over multiple threads.

## Optimization
Thankfully, a very simple optimization can massively reduce this problem space, alignment. We assume that our image is going to be aligned on a 4 Kb page boundary (e.g. the low 12-bits of the address must be zero). We can therefore reason:

```
IF:
base = pointer - offset
THEN:
(base & 0xFFF) = (pointer & 0xFFF) - (offset & 0xFFF)
```

Therefore:
```
IF:
(base & 0xFFF) = 0
THEN:
(pointer & 0xFFF) = (offset & 0xFFF)
```

Therefore, we need only compare `strings` and `pointers` if their low 12-bits (their page offsets) match. Therefore, assuming an even distribution of the low 12-bits of `strings` and `pointers`, we can calculate the number of necessary calculations as follows:

```
For each possible page offset:
(100,000 strings / 4096) * (1,000,000 pointers / 4096) ~=  5960 comparisons

Thus:
5960 comparisons * 4096 page offsets = 24,414,062 comparisons
```

Note that given our base address is non-negative, we should ignore any comparisons where `pointer` `<` `offset`. 

# Implementation

The [implementation](src/main.rs) is split into 4 functions:
1. `get_strings_by_page_offset`
2. `get_addresses_by_page_offset`
3. `get_base_address`
4. `main`

## `get_strings_by_page_offset`
This function takes the image file as input and splits it into several chunks for parallel processing. If the chunks were simply adjacent to each other, then a string could potentially overlap a chunk boundary. Therefore, we break the input into overlapping chunks where the overlap is the size of the largest string to search for minus one.

Our function then uses [`rayon`](https://crates.io/crates/rayon) to process each of the chunks in parallel using a `Regex` iterator to search for matches. The offsets of these `strings` are stored into a [`DashSet`](https://docs.rs/dashmap/latest/dashmap/struct.DashSet.html) (a parallel `Set` type implemented by the `dashmap` crate).

Lastly, we build (again in parallel) an [`DashMap`](https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html) to hold this data. The `string` offsets are stored in `Vec`tors indexed by their page offset. Note that we use the [`take_any`](https://docs.rs/rayon/latest/rayon/iter/trait.ParallelIterator.html#method.take_any) function of the [`ParallelIterator`](https://docs.rs/rayon/latest/rayon/iter/trait.ParallelIterator.html) to sample our data. Whilst this doesn't give us a random sample, it is very performant and empirical evidence seems to show it is sufficient.

## `get_addresses_by_page_offset`
Rather than interpreting the image as a byte array, this function interprets it as an array of `words`. This array of words is split into chunks by [`rayon`](https://crates.io/crates/rayon) and all non-zero `words` are collected into a [`DashSet`](https://docs.rs/dashmap/latest/dashmap/struct.DashSet.html).

Again, this set is processed (in parallel) into a [`DashMap`](https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html). The `words` are stored in `Vec`tors indexed by their page offset. Again, we use [`take_any`](https://docs.rs/rayon/latest/rayon/iter/trait.ParallelIterator.html#method.take_any) to sample our data.

## `get_base_address`
This function is responsible for processing the [`DashMap`](https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html)s built by `get_strings_by_page_offset` and `get_addresses_by_page_offset`. It processes each of the keys of the `strings` index and looks up the corresponding entry in the `addresses` index. Then for each combination of `string` and `address` from the lists, it first checks the `address` is greater than or equal to the `string` offset (recall otherwise it would indicate a negative base address) and discounts the others. Then it subtracts the `string` offset from the `address` to find a candidate `base address`. This `base address` is then inserted into a [`DashMap`](https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html). This [`DashMap`](https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html) uses the `base address` as the key and stores a simple counter of occurences as its value.

We then process this [`DashMap`](https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html) to filter out any entries which occur only once. This dramatically reduces its size, improving the performance of the next step. Next we extract the filtered data into a `Vec`tor of key/value pairs and sort them by the value (the number of occurences). We then print the frequency of the top `10` candidate `base addresses` (to allow the user to get an idea of how much a margin there was beteween the most frequent base address and the other candidates) before returning the most frequently found address as our result.

## `main`
This function is responsible for parsing the arguments passed by the user on the commandline using [`clap`](https://crates.io/crates/clap) and it's `derive` feature to allow us to represent the user command line input as a `struct`. It then uses [`memmap2`](https://docs.rs/memmap2/latest/memmap2/) to map our input file before passing it's data to the remaining functions for analysis and printing our results.