use gungraun::prelude::*;
use std::hint;
use std::io::Write;

macro_rules! benchmarks {
    (
        $itoa:ident,
        $std_fmt:ident,
        $setup_std_fmt:ident,
        $ty:ty,
        $($id:ident => $value:expr),+ $(,)?
    ) => {
        fn $setup_std_fmt(value: $ty) -> (Vec<u8>, $ty) {
            (
                Vec::with_capacity(<$ty as itoa::Integer>::MAX_STR_LEN),
                value,
            )
        }

        #[library_benchmark]
        $(
            #[bench::$id($value)]
        )+
        fn $itoa(int: $ty) -> usize {
            let mut buf = itoa::Buffer::new();
            let formatted = buf.format(hint::black_box(int));
            hint::black_box(formatted).len()
        }

        #[library_benchmark]
        $(
            #[bench::$id(args = ($value), setup = $setup_std_fmt)]
        )+
        fn $std_fmt((mut buf, int): (Vec<u8>, $ty)) -> usize {
            write!(&mut buf, "{}", hint::black_box(int)).unwrap();
            hint::black_box(buf.as_slice()).len()
        }
    };
}

benchmarks!(
    itoa_u64,
    std_fmt_u64,
    setup_std_fmt_u64,
    u64,
    zero => 0,
    half => u64::from(u32::MAX),
    max => u64::MAX,
);

benchmarks!(
    itoa_i16,
    std_fmt_i16,
    setup_std_fmt_i16,
    i16,
    zero => 0,
    min => i16::MIN,
);

benchmarks!(
    itoa_u128,
    std_fmt_u128,
    setup_std_fmt_u128,
    u128,
    zero => 0,
    max => u128::MAX,
);

library_benchmark_group!(
    name = benches;
    benchmarks =
        itoa_u64,
        std_fmt_u64,
        itoa_i16,
        std_fmt_i16,
        itoa_u128,
        std_fmt_u128
);

main!(library_benchmark_groups = benches);
