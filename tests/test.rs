#![allow(non_snake_case)]

macro_rules! test {
    ($($name:ident($value:expr, $expected:expr))*) => {
        $(
            #[test]
            fn $name() {
                let mut buffer = itoa::Buffer::new();
                let s = buffer.format($value);
                assert_eq!(s, $expected);
            }
        )*
    }
}

test! {
    test_u64_0(0u64, "0")
    test_u64_half(u64::from(u32::MAX), "4294967295")
    test_u64_max(u64::MAX, "18446744073709551615")
    test_i64_min(i64::MIN, "-9223372036854775808")

    test_i16_0(0i16, "0")
    test_i16_min(i16::MIN, "-32768")

    test_u128_0(0u128, "0")
    test_u128_max(u128::MAX, "340282366920938463463374607431768211455")
    test_i128_min(i128::MIN, "-170141183460469231731687303715884105728")
    test_i128_max(i128::MAX, "170141183460469231731687303715884105727")
}

#[test]
fn test_max_str_len() {
    use itoa::Integer as _;

    assert_eq!(i8::MAX_STR_LEN, 4);
    assert_eq!(u8::MAX_STR_LEN, 3);
    assert_eq!(i16::MAX_STR_LEN, 6);
    assert_eq!(u16::MAX_STR_LEN, 5);
    assert_eq!(i32::MAX_STR_LEN, 11);
    assert_eq!(u32::MAX_STR_LEN, 10);
    assert_eq!(i64::MAX_STR_LEN, 20);
    assert_eq!(u64::MAX_STR_LEN, 20);
    assert_eq!(i128::MAX_STR_LEN, 40);
    assert_eq!(u128::MAX_STR_LEN, 39);
}

#[cfg(not(feature = "no-panic"))]
fn check<I>(value: I)
where
    I: itoa::Integer + std::fmt::Display + Copy,
{
    let expected = value.to_string();
    let mut buffer = itoa::Buffer::new();
    assert_eq!(buffer.format(value), expected);
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg(not(feature = "no-panic"))]
fn exhaustive_16_bit() {
    for value in u16::MIN..=u16::MAX {
        check(value);
        check(i16::from_ne_bytes(value.to_ne_bytes()));
    }
}

#[test]
#[cfg(not(feature = "no-panic"))]
fn powers_of_ten_boundaries() {
    let mut power = 1u128;
    loop {
        check(power);
        check(power - 1);
        if let Some(next) = power.checked_add(1) {
            check(next);
        }

        if power <= i128::MAX as u128 {
            let signed = i128::try_from(power).expect("range checked");
            check(signed);
            check(-signed);
            check(signed - 1);
            check(-signed + 1);
            if let Some(next) = signed.checked_add(1) {
                check(next);
                check(-next);
            }
        }

        let Some(next) = power.checked_mul(10) else {
            break;
        };
        power = next;
    }
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg(not(feature = "no-panic"))]
fn sampled_wide_integers() {
    let mut state = 0x243f_6a88_85a3_08d3_u64;
    for _ in 0..10_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;

        check(state);
        check(i64::from_ne_bytes(state.to_ne_bytes()));
        let low = u32::try_from(state & u64::from(u32::MAX)).expect("masked to u32");
        check(low);
        check(i32::from_ne_bytes(low.to_ne_bytes()));

        let wide = u128::from(state) * 0x9e37_79b9_7f4a_7c15_u128 + u128::from(!state);
        check(wide);
        check(i128::from_ne_bytes(wide.to_ne_bytes()));
    }

    check(u8::MIN);
    check(u8::MAX);
    check(i8::MIN);
    check(i8::MAX);
    check(usize::MIN);
    check(usize::MAX);
    check(isize::MIN);
    check(isize::MAX);
}
