#![feature(core)]
#![cfg_attr(test, feature(test))]

mod number_streams;

use number_streams::NumberStreams;

const CHUNK_SIZE: usize = 32;

const PRIME_1: u64 = 11400714785074694791;
const PRIME_2: u64 = 14029467366897019727;
const PRIME_3: u64 = 1609587929392839161;
const PRIME_4: u64 = 9650029242287828579;
const PRIME_5: u64 = 2870177450012600261;

#[derive(Copy,Clone,PartialEq)]
struct XxCore {
    v1: u64,
    v2: u64,
    v3: u64,
    v4: u64,
}

#[derive(Debug,Copy,Clone)]
pub struct XxHash {
    total_len: u64,
    seed: u64,
    core: XxCore,
    buffer: [u8; CHUNK_SIZE],
    buffer_usage: usize,
}

impl XxCore {
    fn from_seed(seed: u64) -> XxCore {
        XxCore {
            v1: seed.wrapping_add(PRIME_1).wrapping_add(PRIME_2),
            v2: seed.wrapping_add(PRIME_2),
            v3: seed,
            v4: seed.wrapping_sub(PRIME_1),
        }
    }

    #[inline(always)]
    fn ingest_chunks<I>(&mut self, values: I)
        where I: Iterator<Item=u64>
    {
        #[inline(always)]
        fn ingest_one_number(mut current_value: u64, mut value: u64) -> u64 {
            value = value.wrapping_mul(PRIME_2);
            current_value = current_value.wrapping_add(value);
            current_value = current_value.rotate_left(31);
            current_value.wrapping_mul(PRIME_1)
        };

        // By drawing these out, we can avoid going back and forth to
        // memory. It only really helps for large files, when we need
        // to iterate multiple times here.

        let mut v1 = self.v1;
        let mut v2 = self.v2;
        let mut v3 = self.v3;
        let mut v4 = self.v4;

        let mut values = values.peekable();

        while values.peek().is_some() {
            v1 = ingest_one_number(v1, values.next().unwrap());
            v2 = ingest_one_number(v2, values.next().unwrap());
            v3 = ingest_one_number(v3, values.next().unwrap());
            v4 = ingest_one_number(v4, values.next().unwrap());
        }

        self.v1 = v1;
        self.v2 = v2;
        self.v3 = v3;
        self.v4 = v4;
    }

    #[inline(always)]
    fn finish(&self) -> u64 {
        // TODO: The original code pulls out local vars for
        // v[1234], presumably for performance
        // reasons. Investigate.

        let mut hash;

        hash =                   self.v1.rotate_left( 1);
        hash = hash.wrapping_add(self.v2.rotate_left( 7));
        hash = hash.wrapping_add(self.v3.rotate_left(12));
        hash = hash.wrapping_add(self.v4.rotate_left(18));

        #[inline(always)]
        fn mix_one(mut hash: u64, mut value: u64) -> u64 {
            value = value.wrapping_mul(PRIME_2);
            value = value.rotate_left(31);
            value = value.wrapping_mul(PRIME_1);
            hash ^= value;
            hash = hash.wrapping_mul(PRIME_1);
            hash.wrapping_add(PRIME_4)
        }

        hash = mix_one(hash, self.v1);
        hash = mix_one(hash, self.v2);
        hash = mix_one(hash, self.v3);
        hash = mix_one(hash, self.v4);

        hash
    }
}

impl std::fmt::Debug for XxCore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(
            f, "XxCore {{ {:016x} {:016x} {:016x} {:016x} }}",
            self.v1, self.v2, self.v3, self.v4
        )
    }
}

impl XxHash {
    pub fn from_seed(seed: u64) -> XxHash {
        XxHash {
            total_len: 0,
            seed: seed,
            core: XxCore::from_seed(seed),
            buffer: [0; CHUNK_SIZE],
            buffer_usage: 0,
        }
    }
}

impl XxHash {
    pub fn write(&mut self, bytes: &[u8]) {
        let mut bytes = bytes;

        self.total_len += bytes.len() as u64;

        // Even with new data, we still don't have a full buffer. Wait
        // until we have a full buffer.
        if self.buffer_usage + bytes.len() < self.buffer.len() {
            unsafe {
                let tail = self.buffer.as_mut_ptr().offset(self.buffer_usage as isize);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), tail, bytes.len());
            }
            self.buffer_usage += bytes.len();
            return;
        }

        // Some data left from previous update. Fill the buffer and
        // consume it first.
        if self.buffer_usage > 0 {
            let bytes_to_use = self.buffer.len() - self.buffer_usage;
            let (to_use, leftover) = bytes.split_at(bytes_to_use);

            unsafe {
                let tail = self.buffer.as_mut_ptr().offset(self.buffer_usage as isize);
                std::ptr::copy_nonoverlapping(to_use.as_ptr(), tail, bytes_to_use);
            }

            let (iter, _) = self.buffer.u64_stream();

            self.core.ingest_chunks(iter);

            bytes = leftover;
            self.buffer_usage = 0;
        }

        // Consume the input data in large chunks
        if bytes.len() >= CHUNK_SIZE {
            let (iter, leftover) = bytes.u64_stream();
            self.core.ingest_chunks(iter);
            bytes = leftover;
        }

        // Save any leftover data for the next call
        if bytes.len() > 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.buffer.as_mut_ptr(), bytes.len());
            }
            self.buffer_usage = bytes.len();
        }
    }

    pub fn finish(&self) -> u64 {
        let mut hash;

        // We have processed at least one full chunk
        if self.total_len >= CHUNK_SIZE as u64 {
            hash = self.core.finish();
        } else {
            hash = self.seed.wrapping_add(PRIME_5);
        }

        hash = hash.wrapping_add(self.total_len);

        let buffered = &self.buffer[..self.buffer_usage];
        let (buffered_u64s, buffered) = buffered.u64_stream();

        for mut k1 in buffered_u64s {
            k1 = k1.wrapping_mul(PRIME_2);
            k1 = k1.rotate_left(31);
            k1 = k1.wrapping_mul(PRIME_1);
            hash ^= k1;
            hash = hash.rotate_left(27);
            hash = hash.wrapping_mul(PRIME_1);
            hash = hash.wrapping_add(PRIME_4);
        }

        let (buffered_u32s, buffered) = buffered.u32_stream();

        for k1 in buffered_u32s {
            let k1 = (k1 as u64).wrapping_mul(PRIME_1);
            hash ^= k1;
            hash = hash.rotate_left(23);
            hash = hash.wrapping_mul(PRIME_2);
            hash = hash.wrapping_add(PRIME_3);
        }

        for buffered_u8 in buffered {
            let k1 = (*buffered_u8 as u64).wrapping_mul(PRIME_5);
            hash ^= k1;
            hash = hash.rotate_left(11);
            hash = hash.wrapping_mul(PRIME_1);
        }

        // The final intermixing
        hash ^= hash.wrapping_shr(33);
        hash = hash.wrapping_mul(PRIME_2);
        hash ^= hash.wrapping_shr(29);
        hash = hash.wrapping_mul(PRIME_3);
        hash ^= hash.wrapping_shr(32);

        hash
    }
}

#[cfg(test)]
mod test {
    use super::XxHash;

    #[test]
    fn ingesting_byte_by_byte_is_equivalent_to_large_chunks() {
        let bytes: Vec<_> = (0..32).map(|_| 0).collect();

        let mut byte_by_byte = XxHash::from_seed(0);
        for byte in bytes.chunks(1) {
            byte_by_byte.write(byte);
        }

        let mut one_chunk = XxHash::from_seed(0);
        one_chunk.write(&bytes);

        assert_eq!(byte_by_byte.core, one_chunk.core);
    }

    #[test]
    fn hash_of_nothing_matches_c_implementation() {
        let mut hasher = XxHash::from_seed(0);
        hasher.write(&[]);
        assert_eq!(hasher.finish(), 0xef46db3751d8e999);
    }

    #[test]
    fn hash_of_single_byte_matches_c_implementation() {
        let mut hasher = XxHash::from_seed(0);
        hasher.write(&[42]);
        assert_eq!(hasher.finish(), 0x0a9edecebeb03ae4);
    }

    #[test]
    fn hash_of_multiple_bytes_matches_c_implementation() {
        let mut hasher = XxHash::from_seed(0);
        hasher.write(b"Hello, world!\0");
        assert_eq!(hasher.finish(), 0x7b06c531ea43e89f);
    }

    #[test]
    fn hash_of_multiple_chunks_matches_c_implementation() {
        let bytes: Vec<_> = (0..100).collect();
        let mut hasher = XxHash::from_seed(0);
        hasher.write(&bytes);
        assert_eq!(hasher.finish(), 0x6ac1e58032166597);
    }

    #[test]
    fn hash_with_different_seed_matches_c_implementation() {
        let mut hasher = XxHash::from_seed(0xae0543311b702d91);
        hasher.write(&[]);
        assert_eq!(hasher.finish(), 0x4b6a04fcdf7a4672);
    }

    #[test]
    fn hash_with_different_seed_and_multiple_chunks_matches_c_implementation() {
        let bytes: Vec<_> = (0..100).collect();
        let mut hasher = XxHash::from_seed(0xae0543311b702d91);
        hasher.write(&bytes);
        assert_eq!(hasher.finish(), 0x567e355e0682e1f1);
    }
}

#[cfg(test)]
mod bench {
    extern crate test;

    use super::XxHash;

    #[inline(always)]
    fn straight_line_slice_bench(b: &mut test::Bencher, len: usize) {
        let bytes: Vec<_> = (0..100).cycle().take(len).collect();
        b.bytes = bytes.len() as u64;
        b.iter(|| {
            let mut hasher = XxHash::from_seed(0);
            hasher.write(&bytes);
            hasher.finish()
        });
    }

    #[bench]
    fn straight_line_megabyte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 1024*1024);
    }

    #[bench]
    fn straight_line_1024_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 1024);
    }

    #[bench]
    fn straight_line_512_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 512);
    }

    #[bench]
    fn straight_line_256_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 256);
    }

    #[bench]
    fn straight_line_128_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 128);
    }

    #[bench]
    fn straight_line_32_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 32);
    }

    #[bench]
    fn straight_line_16_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 16);
    }

    #[bench]
    fn straight_line_4_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 4);
    }

    #[bench]
    fn straight_line_1_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 1);
    }

    #[bench]
    fn straight_line_0_byte(b: &mut test::Bencher) {
        straight_line_slice_bench(b, 0);
    }
}