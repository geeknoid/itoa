//! [![github]](https://github.com/dtolnay/itoa)&ensp;[![crates-io]](https://crates.io/crates/itoa)&ensp;[![docs-rs]](https://docs.rs/itoa)
//!
//! [github]: https://img.shields.io/badge/github-8da0cb?style=for-the-badge&labelColor=555555&logo=github
//! [crates-io]: https://img.shields.io/badge/crates.io-fc8d62?style=for-the-badge&labelColor=555555&logo=rust
//! [docs-rs]: https://img.shields.io/badge/docs.rs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs
//!
//! <br>
//!
//! This crate provides a fast conversion of integer primitives to decimal
//! strings. The implementation comes straight from [libcore] but avoids the
//! performance penalty of going through [`core::fmt::Formatter`].
//!
//! See also [`zmij`] for printing floating point primitives.
//!
//! [libcore]: https://github.com/rust-lang/rust/blob/1.92.0/library/core/src/fmt/num.rs#L190-L253
//! [`zmij`]: https://github.com/dtolnay/zmij
//!
//! # Example
//!
//! ```
//! fn main() {
//!     let mut buffer = itoa::Buffer::new();
//!     let printed = buffer.format(128u64);
//!     assert_eq!(printed, "128");
//! }
//! ```
//!
//! # Performance
//!
//! The [itoa-benchmark] compares this library and other Rust integer formatting
//! implementations across a range of integer sizes. The vertical axis in this
//! chart shows nanoseconds taken by a single execution of
//! `itoa::Buffer::new().format(value)` so a lower result indicates a faster
//! library.
//!
//! [itoa-benchmark]: https://github.com/dtolnay/itoa-benchmark
//!
//! ![performance](https://raw.githubusercontent.com/dtolnay/itoa/master/itoa-benchmark.png)

#![doc(html_root_url = "https://docs.rs/itoa/1.0.18")]
#![no_std]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::expl_impl_clone_on_copy,
    clippy::identity_op,
    clippy::items_after_statements,
    clippy::must_use_candidate,
    clippy::needless_doctest_main,
    clippy::unreadable_literal
)]

mod u128_ext;

use core::hint;
use core::mem::{self, MaybeUninit};
use core::str;
#[cfg(feature = "no-panic")]
use no_panic::no_panic;

/// A correctly sized stack allocation for the formatted integer to be written
/// into.
///
/// # Example
///
/// ```
/// let mut buffer = itoa::Buffer::new();
/// let printed = buffer.format(1234);
/// assert_eq!(printed, "1234");
/// ```
pub struct Buffer {
    bytes: [MaybeUninit<u8>; i128::MAX_STR_LEN],
}

impl Default for Buffer {
    #[inline]
    fn default() -> Buffer {
        Buffer::new()
    }
}

impl Copy for Buffer {}

#[allow(clippy::non_canonical_clone_impl)]
impl Clone for Buffer {
    #[inline]
    fn clone(&self) -> Self {
        Buffer::new()
    }
}

impl Buffer {
    /// This is a cheap operation; you don't need to worry about reusing buffers
    /// for efficiency.
    #[inline]
    #[cfg_attr(feature = "no-panic", no_panic)]
    pub fn new() -> Buffer {
        let bytes = [MaybeUninit::<u8>::uninit(); i128::MAX_STR_LEN];
        Buffer { bytes }
    }

    /// Print an integer into this buffer and return a reference to its string
    /// representation within the buffer.
    #[cfg_attr(feature = "no-panic", no_panic)]
    pub fn format<I: Integer>(&mut self, i: I) -> &str {
        let buf_ptr = self.bytes.as_mut_ptr().cast::<I::Buffer>();
        let string = i.write(unsafe { &mut *buf_ptr });
        if string.len() > I::MAX_STR_LEN {
            unsafe { hint::unreachable_unchecked() };
        }
        string
    }
}

/// An integer that can be written into an [`itoa::Buffer`][Buffer].
///
/// This trait is sealed and cannot be implemented for types outside of itoa.
pub trait Integer: private::Sealed {
    /// The maximum length of string that formatting an integer of this type can
    /// produce on the current target platform.
    const MAX_STR_LEN: usize;
}

// Seal to prevent downstream implementations of the Integer trait.
mod private {
    #[doc(hidden)]
    pub trait Sealed: Copy {
        #[doc(hidden)]
        type Buffer: 'static;
        fn write(self, buf: &mut Self::Buffer) -> &str;
    }
}

macro_rules! impl_Integer {
    ($Signed:ident, $Unsigned:ident) => {
        const _: () = {
            assert!($Signed::MIN < 0, "need signed");
            assert!($Unsigned::MIN == 0, "need unsigned");
            assert!($Signed::BITS == $Unsigned::BITS, "need counterparts");
        };

        impl Integer for $Unsigned {
            const MAX_STR_LEN: usize = $Unsigned::MAX.ilog10() as usize + 1;
        }

        impl private::Sealed for $Unsigned {
            type Buffer = [MaybeUninit<u8>; Self::MAX_STR_LEN];

            #[inline]
            #[cfg_attr(feature = "no-panic", no_panic)]
            fn write(self, buf: &mut Self::Buffer) -> &str {
                let offset = Unsigned::fmt(self, buf);
                // SAFETY: Starting from `offset`, all elements of the slice have been set.
                unsafe { slice_buffer_to_str(buf, offset) }
            }
        }

        impl Integer for $Signed {
            const MAX_STR_LEN: usize = $Signed::MAX.ilog10() as usize + 2;
        }

        impl private::Sealed for $Signed {
            type Buffer = [MaybeUninit<u8>; Self::MAX_STR_LEN];

            #[inline]
            #[cfg_attr(feature = "no-panic", no_panic)]
            fn write(self, buf: &mut Self::Buffer) -> &str {
                let mut offset = Self::MAX_STR_LEN - $Unsigned::MAX_STR_LEN;
                // SAFETY: `offset == Self::MAX_STR_LEN - $Unsigned::MAX_STR_LEN`,
                // so the `$Unsigned::MAX_STR_LEN` bytes starting at `offset` are
                // exactly the tail of `buf` and form a valid unsigned buffer.
                let unsigned_buf = unsafe {
                    &mut *buf
                        .as_mut_ptr()
                        .add(offset)
                        .cast::<<$Unsigned as private::Sealed>::Buffer>()
                };
                offset += Unsigned::fmt(self.unsigned_abs(), unsigned_buf);
                if self < 0 {
                    offset -= 1;
                    // SAFETY: `offset` indexes the byte immediately before the
                    // digits, which is within `buf`.
                    unsafe { assert_unchecked(offset < buf.len()) };
                    buf[offset].write(b'-');
                }
                // SAFETY: Starting from `offset`, all elements of the slice have been set.
                unsafe { slice_buffer_to_str(buf, offset) }
            }
        }
    };
}

impl_Integer!(i8, u8);
impl_Integer!(i16, u16);
impl_Integer!(i32, u32);
impl_Integer!(i64, u64);
impl_Integer!(i128, u128);

macro_rules! impl_Integer_size {
    ($t:ty as $primitive:ident #[cfg(target_pointer_width = $width:literal)]) => {
        #[cfg(target_pointer_width = $width)]
        impl Integer for $t {
            const MAX_STR_LEN: usize = <$primitive as Integer>::MAX_STR_LEN;
        }

        #[cfg(target_pointer_width = $width)]
        impl private::Sealed for $t {
            type Buffer = <$primitive as private::Sealed>::Buffer;

            #[inline]
            #[cfg_attr(feature = "no-panic", no_panic)]
            fn write(self, buf: &mut Self::Buffer) -> &str {
                (self as $primitive).write(buf)
            }
        }
    };
}

impl_Integer_size!(isize as i16 #[cfg(target_pointer_width = "16")]);
impl_Integer_size!(usize as u16 #[cfg(target_pointer_width = "16")]);
impl_Integer_size!(isize as i32 #[cfg(target_pointer_width = "32")]);
impl_Integer_size!(usize as u32 #[cfg(target_pointer_width = "32")]);
impl_Integer_size!(isize as i64 #[cfg(target_pointer_width = "64")]);
impl_Integer_size!(usize as u64 #[cfg(target_pointer_width = "64")]);

#[repr(C, align(2))]
struct DecimalPairs([u8; 200]);

// The string of all two-digit numbers in range 00..99 is used as a lookup table.
static DECIMAL_PAIRS: DecimalPairs = DecimalPairs(
    *b"0001020304050607080910111213141516171819\
       2021222324252627282930313233343536373839\
       4041424344454647484950515253545556575859\
       6061626364656667686970717273747576777879\
       8081828384858687888990919293949596979899",
);

// Returns {value / 100, value % 100} correct for values of up to 4 digits.
fn divmod100(value: u32) -> (u32, u32) {
    debug_assert!(value < 10_000);
    const EXP: u32 = 19; // 19 is faster or equal to 12 even for 3 digits.
    const SIG: u32 = (1 << EXP) / 100 + 1;
    let div = (value * SIG) >> EXP; // value / 100
    (div, value - div * 100)
}

/// Informs the optimizer that `cond` always holds, so that it can discard the
/// bounds checks and panic paths that depend on it.
///
/// This is [`core::hint::assert_unchecked`], which is newer than this crate's
/// minimum supported Rust version.
///
/// # Safety
///
/// `cond` must be true.
#[inline(always)]
unsafe fn assert_unchecked(cond: bool) {
    debug_assert!(cond);
    if !cond {
        // SAFETY: The caller guarantees that `cond` holds.
        unsafe { hint::unreachable_unchecked() };
    }
}

/// Writes `pair` as exactly two decimal digits into `buf[offset..offset + 2]`,
/// zero padded (for example 7 becomes `"07"`).
///
/// # Safety
///
/// `pair` must be below 100, and both `offset` and `offset + 1` must be valid
/// indices into `buf`.
#[inline(always)]
unsafe fn write_pair(buf: &mut [MaybeUninit<u8>], offset: usize, pair: u32) {
    // SAFETY: These are this function's caller-provided invariants. They let
    // the indexing below compile without bounds checks. Each index is asserted
    // individually because `offset + n < buf.len()` alone would also hold for
    // an `offset` whose addition wraps around.
    unsafe {
        assert_unchecked(pair < 100);
        assert_unchecked(offset + 0 < buf.len());
        assert_unchecked(offset + 1 < buf.len());
    }

    let pair = pair as usize;
    buf[offset + 0].write(DECIMAL_PAIRS.0[pair * 2 + 0]);
    buf[offset + 1].write(DECIMAL_PAIRS.0[pair * 2 + 1]);
}

/// Writes `quad` as exactly four decimal digits into `buf[offset..offset + 4]`,
/// zero padded (for example 42 becomes `"0042"`).
///
/// # Safety
///
/// `quad` must be below 10_000, and `offset..offset + 4` must all be valid
/// indices into `buf`.
#[inline(always)]
unsafe fn write_quad(buf: &mut [MaybeUninit<u8>], offset: usize, quad: u32) {
    // SAFETY: These are this function's caller-provided invariants.
    unsafe {
        assert_unchecked(quad < 10_000);
        assert_unchecked(offset + 0 < buf.len());
        assert_unchecked(offset + 1 < buf.len());
        assert_unchecked(offset + 2 < buf.len());
        assert_unchecked(offset + 3 < buf.len());
    }

    let (pair1, pair2) = divmod100(quad);
    // SAFETY: `quad` is below 10_000, so both halves are below 100.
    unsafe { assert_unchecked(pair1 < 100 && pair2 < 100) };

    // SAFETY: Both pairs are below 100 and all four indices are in bounds.
    unsafe {
        write_pair(buf, offset + 0, pair1);
        write_pair(buf, offset + 2, pair2);
    }
}

/// This function converts a slice of ascii characters into a `&str` starting
/// from `offset`.
///
/// # Safety
///
/// `buf` content starting from `offset` index MUST BE initialized and MUST BE
/// ascii characters.
#[cfg_attr(feature = "no-panic", no_panic)]
unsafe fn slice_buffer_to_str(buf: &[MaybeUninit<u8>], offset: usize) -> &str {
    // SAFETY: `offset` is always included between 0 and `buf`'s length.
    let written = unsafe { buf.get_unchecked(offset..) };
    // SAFETY: (`assume_init_ref`) All buf content since offset is set.
    // SAFETY: (`from_utf8_unchecked`) Writes use ASCII from the lookup table exclusively.
    unsafe { str::from_utf8_unchecked(&*(written as *const [MaybeUninit<u8>] as *const [u8])) }
}

trait Unsigned: Integer {
    fn fmt(self, buf: &mut Self::Buffer) -> usize;
}

macro_rules! impl_Unsigned {
    ($Unsigned:ident) => {
        impl Unsigned for $Unsigned {
            #[cfg_attr(feature = "no-panic", no_panic)]
            fn fmt(self, buf: &mut Self::Buffer) -> usize {
                // Count the number of bytes in buf that are not initialized.
                let mut offset = buf.len();
                if self == 0 {
                    offset -= 1;
                    // SAFETY: Every integer buffer has room for at least one digit.
                    unsafe { assert_unchecked(offset < buf.len()) };
                    buf[offset].write(b'0');
                    return offset;
                }
                // Consume the least-significant decimals from a working copy.
                let mut remain = self;

                // Format per four digits from the lookup table.
                // Four digits need a 16-bit $Unsigned or wider. The fallible
                // conversions cannot panic: for `u8` the `size_of` guard makes
                // the loop dead, and `unwrap_or` avoids a panic path there; for
                // wider types the constants convert successfully.
                while mem::size_of::<Self>() > 1
                    && remain > 999.try_into().unwrap_or(Self::MAX)
                {
                    offset -= 4;

                    // pull two pairs
                    let scale: Self = 1_00_00.try_into().unwrap_or(1);
                    let quad = remain % scale;
                    remain /= scale;
                    // SAFETY: `quad` is a remainder modulo 10_000. Every four
                    // digits written consume four of the `MAX_STR_LEN` bytes
                    // this type's buffer is sized for, so `offset` was just
                    // decremented into bounds and never drops below zero.
                    unsafe { write_quad(buf.as_mut_slice(), offset, quad as u32) };
                }

                // Format per two digits from the lookup table.
                if remain > 9 {
                    offset -= 2;

                    let (last, pair) = divmod100(remain as u32);
                    remain = last as Self;
                    // SAFETY: `pair` is a remainder modulo 100, and `offset` was
                    // just decremented by 2 without dropping below zero.
                    unsafe { write_pair(buf.as_mut_slice(), offset, pair) };
                }

                // Format the last remaining digit, if any.
                if remain != 0 || self == 0 {
                    offset -= 1;

                    let last = remain as u8 & 15;
                    // SAFETY: `offset` was just decremented by 1 and never drops
                    // below zero, so it is a valid index into `buf`.
                    unsafe { assert_unchecked(offset < buf.len()) };
                    buf[offset].write(b'0' + last);
                    // not used: remain = 0;
                }

                offset
            }
        }
    };
}

impl_Unsigned!(u8);
#[cfg(not(all(target_feature = "sse4.1", target_feature = "lzcnt")))]
impl_Unsigned!(u16);
impl_Unsigned!(u32);
#[cfg(not(all(target_feature = "sse4.1", target_feature = "lzcnt")))]
impl_Unsigned!(u64);

#[cfg(all(
    target_feature = "sse4.1",
    target_feature = "lzcnt"
))]
#[inline]
#[cfg_attr(feature = "no-panic", no_panic)]
fn to_bcd4(abcd: u16) -> u32 {
    let abcd = u32::from(abcd);
    let ab_cd = abcd + (0x10000 - 100) * ((abcd * 0x147b) >> 19);
    ab_cd + (0x100 - 10) * (((ab_cd * 0x67) >> 10) & 0xf000f)
}

#[cfg(all(
    target_feature = "sse4.1",
    target_feature = "lzcnt"
))]
impl Unsigned for u16 {
    #[cfg_attr(feature = "no-panic", no_panic)]
    fn fmt(self, buf: &mut Self::Buffer) -> usize {
        if self == 0 {
            buf[4].write(b'0');
            return 4;
        }
        if self >= 10_000 {
            let high = self / 10_000;
            let bcd = to_bcd4(self % 10_000);
            buf[0].write(b'0' + high as u8);
            // SAFETY: Bytes 1..5 are within the five-byte output buffer.
            unsafe {
                buf.as_mut_ptr()
                    .add(1)
                    .cast::<u32>()
                    .write_unaligned((bcd | 0x30303030).to_be());
            }
            return 0;
        }
        let bcd = to_bcd4(self);
        let leading_zeros = bcd.leading_zeros() as usize / 8;
        // SAFETY: Bytes 1..5 are within the five-byte output buffer.
        unsafe {
            buf.as_mut_ptr()
                .add(1)
                .cast::<u32>()
                .write_unaligned((bcd | 0x30303030).to_be());
        }
        1 + leading_zeros
    }
}

#[cfg(all(
    target_feature = "sse4.1",
    target_feature = "lzcnt"
))]
impl Unsigned for u64 {
    #[cfg_attr(feature = "no-panic", no_panic)]
    fn fmt(self, buf: &mut Self::Buffer) -> usize {
        if self == 0 {
            buf[19].write(b'0');
            return 19;
        }
        let out = buf.as_mut_ptr().cast::<u32>();

        if self >= 10_000_000_000_000_000 {
            let top = self / 10_000_000_000_000_000;
            let hi = (self % 10_000_000_000_000_000 / 100_000_000) as u32;
            let lo = (self % 100_000_000) as u32;
            let bcd_top = to_bcd4(top as u16);
            let bcd_hi_hi = to_bcd4((hi / 10_000) as u16);
            let bcd_hi_lo = to_bcd4((hi % 10_000) as u16);
            let bcd_lo_hi = to_bcd4((lo / 10_000) as u16);
            let bcd_lo_lo = to_bcd4((lo % 10_000) as u16);
            let leading_zeros = bcd_top.leading_zeros() as usize / 8;
            // SAFETY: The five writes cover exactly the 20-byte output buffer.
            unsafe {
                out.write_unaligned((bcd_top | 0x30303030).to_be());
                out.add(1).write_unaligned((bcd_hi_hi | 0x30303030).to_be());
                out.add(2).write_unaligned((bcd_hi_lo | 0x30303030).to_be());
                out.add(3).write_unaligned((bcd_lo_hi | 0x30303030).to_be());
                out.add(4).write_unaligned((bcd_lo_lo | 0x30303030).to_be());
            }
            return leading_zeros;
        }

        if self >= 100_000_000 {
            let hi = (self / 100_000_000) as u32;
            let lo = (self % 100_000_000) as u32;
            let bcd_hi_hi = to_bcd4((hi / 10_000) as u16);
            let bcd_hi_lo = to_bcd4((hi % 10_000) as u16);
            let bcd_lo_hi = to_bcd4((lo / 10_000) as u16);
            let bcd_lo_lo = to_bcd4((lo % 10_000) as u16);
            let leading_zeros =
                ((u64::from(bcd_hi_hi) << 32) | u64::from(bcd_hi_lo)).leading_zeros() as usize / 8;
            // SAFETY: The four writes cover bytes 4..20 of the output buffer.
            unsafe {
                out.add(1).write_unaligned((bcd_hi_hi | 0x30303030).to_be());
                out.add(2).write_unaligned((bcd_hi_lo | 0x30303030).to_be());
                out.add(3).write_unaligned((bcd_lo_hi | 0x30303030).to_be());
                out.add(4).write_unaligned((bcd_lo_lo | 0x30303030).to_be());
            }
            return 4 + leading_zeros;
        }

        if self >= 10_000 {
            let bcd_hi = to_bcd4((self / 10_000) as u16);
            let bcd_lo = to_bcd4((self % 10_000) as u16);
            let leading_zeros = bcd_hi.leading_zeros() as usize / 8;
            // SAFETY: The two writes cover bytes 12..20 of the output buffer.
            unsafe {
                out.add(3).write_unaligned((bcd_hi | 0x30303030).to_be());
                out.add(4).write_unaligned((bcd_lo | 0x30303030).to_be());
            }
            return 12 + leading_zeros;
        }

        let bcd = to_bcd4(self as u16);
        let leading_zeros = bcd.leading_zeros() as usize / 8;
        // SAFETY: The write covers bytes 16..20 of the output buffer.
        unsafe {
            out.add(4).write_unaligned((bcd | 0x30303030).to_be());
        }
        16 + leading_zeros
    }
}

impl Unsigned for u128 {
    #[cfg_attr(feature = "no-panic", no_panic)]
    fn fmt(self, buf: &mut Self::Buffer) -> usize {
        // Optimize common-case zero, which would also need special treatment due to
        // its "leading" zero.
        if self == 0 {
            let offset = buf.len() - 1;
            buf[offset].write(b'0');
            return offset;
        }
        // Take the 16 least-significant decimals.
        let (quot_1e16, mod_1e16) = div_rem_1e16(self);
        let (mut remain, mut offset) = if quot_1e16 == 0 {
            (mod_1e16, u128::MAX_STR_LEN)
        } else {
            // Write digits at buf[23..39].
            // SAFETY: `mod_1e16` is a remainder modulo 1e16, and
            // `u128::MAX_STR_LEN - 16 + 16 == buf.len()`.
            unsafe { enc_16lsd::<{ u128::MAX_STR_LEN - 16 }>(buf, mod_1e16) };

            // Take another 16 decimals.
            let (quot2, mod2) = div_rem_1e16(quot_1e16);
            if quot2 == 0 {
                (mod2, u128::MAX_STR_LEN - 16)
            } else {
                // Write digits at buf[7..23].
                // SAFETY: `mod2` is a remainder modulo 1e16, and
                // `u128::MAX_STR_LEN - 32 + 16 <= buf.len()`.
                unsafe { enc_16lsd::<{ u128::MAX_STR_LEN - 32 }>(buf, mod2) };
                #[cfg(all(
                    target_feature = "sse4.1",
                    target_feature = "lzcnt"
                ))]
                return enc_7msd(buf, quot2 as u32);
                // Quot2 has at most 7 decimals remaining after two 1e16 divisions.
                #[cfg(not(all(target_feature = "sse4.1", target_feature = "lzcnt")))]
                (quot2 as u64, u128::MAX_STR_LEN - 32)
            }
        };

        // Format per four digits from the lookup table.
        while remain > 999 {
            offset -= 4;

            // pull two pairs
            let quad = remain % 1_00_00;
            remain /= 1_00_00;
            // SAFETY: `quad` is a remainder modulo 10_000, and the loop above
            // reserves four bytes per iteration within `buf`.
            unsafe { write_quad(buf.as_mut_slice(), offset, quad as u32) };
        }

        // Format per two digits from the lookup table.
        if remain > 9 {
            offset -= 2;

            let (last, pair) = divmod100(remain as u32);
            remain = last as u64;
            // SAFETY: `pair` is a remainder modulo 100, and `offset` was just
            // decremented by 2 without dropping below zero.
            unsafe { write_pair(buf.as_mut_slice(), offset, pair) };
        }

        // Format the last remaining digit, if any.
        if remain != 0 {
            offset -= 1;

            let last = remain as u8 & 15;
            // SAFETY: `offset` was just decremented by 1 and never drops below
            // zero, so it is a valid index into `buf`.
            unsafe { assert_unchecked(offset < buf.len()) };
            buf[offset].write(b'0' + last);
            // not used: remain = 0;
        }
        offset
    }
}

// Encodes the 16 least-significant decimals of n into `buf[OFFSET..OFFSET + 16]`.
//
// # Safety
//
// `n` must be below 1e16, and `OFFSET + 16` must not exceed `buf.len()`.
#[cfg(not(all(target_feature = "sse4.1", target_feature = "lzcnt")))]
#[cfg_attr(feature = "no-panic", no_panic)]
unsafe fn enc_16lsd<const OFFSET: usize>(buf: &mut [MaybeUninit<u8>], n: u64) {
    // SAFETY: These are this function's caller-provided invariants.
    unsafe {
        assert_unchecked(n < 10_000_000_000_000_000);
        assert_unchecked(OFFSET + 16 <= buf.len());
    }

    // Consume the least-significant decimals from a working copy.
    let mut remain = n;

    // Format per four digits from the lookup table.
    for quad_index in (1..4).rev() {
        // pull two pairs
        let quad = remain % 1_00_00;
        remain /= 1_00_00;
        // SAFETY: `quad` is a remainder modulo 10_000, and `quad_index` ranges
        // over 1..4 so the write stays within `OFFSET..OFFSET + 16`.
        unsafe { write_quad(buf, OFFSET + quad_index * 4, quad as u32) };
    }

    // final two pairs
    // SAFETY: `n` is below 1e16, so the remaining decimals are below 10_000,
    // and `OFFSET..OFFSET + 4` are in bounds.
    unsafe { write_quad(buf, OFFSET, remain as u32) };
}

#[cfg(all(
    target_feature = "sse4.1",
    target_feature = "lzcnt"
))]
// # Safety
//
// `n` must be below 1e16, and `OFFSET + 16` must not exceed `buf.len()`.
#[cfg_attr(feature = "no-panic", no_panic)]
unsafe fn enc_16lsd<const OFFSET: usize>(buf: &mut [MaybeUninit<u8>], n: u64) {
    let hi = (n / 100_000_000) as u32;
    let lo = (n % 100_000_000) as u32;
    // SAFETY: Callers provide an offset with at least 16 remaining bytes.
    let out = unsafe { buf.as_mut_ptr().add(OFFSET).cast::<u32>() };
    let bcd_hi_hi = to_bcd4((hi / 10_000) as u16);
    let bcd_hi_lo = to_bcd4((hi % 10_000) as u16);
    let bcd_lo_hi = to_bcd4((lo / 10_000) as u16);
    let bcd_lo_lo = to_bcd4((lo % 10_000) as u16);
    // SAFETY: The four writes cover the 16 bytes starting at OFFSET.
    unsafe {
        out.write_unaligned((bcd_hi_hi | 0x30303030).to_be());
        out.add(1).write_unaligned((bcd_hi_lo | 0x30303030).to_be());
        out.add(2).write_unaligned((bcd_lo_hi | 0x30303030).to_be());
        out.add(3).write_unaligned((bcd_lo_lo | 0x30303030).to_be());
    }
}

#[cfg(all(
    target_feature = "sse4.1",
    target_feature = "lzcnt"
))]
#[cfg_attr(feature = "no-panic", no_panic)]
fn enc_7msd(buf: &mut [MaybeUninit<u8>], n: u32) -> usize {
    debug_assert!(n < 10_000_000);
    if n < 10_000 {
        let bcd = to_bcd4(n as u16);
        let leading_zeros = bcd.leading_zeros() as usize / 8;
        // SAFETY: The write covers bytes 3..7 of the output buffer.
        unsafe {
            buf.as_mut_ptr()
                .add(3)
                .cast::<u32>()
                .write_unaligned((bcd | 0x30303030).to_be());
        }
        3 + leading_zeros
    } else {
        let bcd_hi = to_bcd4((n / 10_000) as u16);
        let bcd_lo = to_bcd4((n % 10_000) as u16);
        let leading_zeros = bcd_hi.leading_zeros() as usize / 8;
        let hi = (bcd_hi | 0x30303030).to_be_bytes();
        // SAFETY: `enc_7msd` writes into `buf[0..7]`; bytes 0..3 are in bounds.
        unsafe {
            buf.get_unchecked_mut(0).write(hi[1]);
            buf.get_unchecked_mut(1).write(hi[2]);
            buf.get_unchecked_mut(2).write(hi[3]);
        }
        // SAFETY: The write covers bytes 3..7 of the output buffer.
        unsafe {
            buf.as_mut_ptr()
                .add(3)
                .cast::<u32>()
                .write_unaligned((bcd_lo | 0x30303030).to_be());
        }
        leading_zeros - 1
    }
}

// Euclidean division plus remainder with constant 1E16 basically consumes 16
// decimals from n.
//
// The integer division algorithm is based on the following paper:
//
//   T. Granlund and P. Montgomery, “Division by Invariant Integers Using Multiplication”
//   in Proc. of the SIGPLAN94 Conference on Programming Language Design and
//   Implementation, 1994, pp. 61–72
//
#[cfg_attr(feature = "no-panic", no_panic)]
fn div_rem_1e16(n: u128) -> (u128, u64) {
    const D: u128 = 1_0000_0000_0000_0000;
    // The check inlines well with the caller flow.
    if n < D {
        return (0, n as u64);
    }

    // These constant values are computed with the CHOOSE_MULTIPLIER procedure
    // from the Granlund & Montgomery paper, using N=128, prec=128 and d=1E16.
    const M_HIGH: u128 = 76624777043294442917917351357515459181;
    const SH_POST: u8 = 51;

    // n.widening_mul(M_HIGH).1 >> SH_POST
    let quot = u128_ext::mulhi(n, M_HIGH) >> SH_POST;
    let rem = n - quot * D;
    (quot, rem as u64)
}
