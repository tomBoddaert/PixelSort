use std::num::NonZero;

pub const U32_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u32>())).unwrap();
pub const U64_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u64>())).unwrap();

pub const fn const_usize_to_u32(value: usize) -> u32 {
    if size_of::<u32>() >= size_of::<usize>() {
        return value as u32;
    }
    if value > u32::MAX as usize {
        panic!();
    }
    value as u32
}

pub const fn const_usize_to_u64(value: usize) -> u64 {
    if size_of::<u64>() >= size_of::<usize>() {
        return value as u64;
    }
    if value > u64::MAX as usize {
        panic!();
    }
    value as u64
}

pub const fn const_u32_to_usize(value: u32) -> usize {
    if size_of::<usize>() >= size_of::<u32>() {
        return value as usize;
    }
    if value > usize::MAX as u32 {
        panic!();
    }
    value as usize
}

pub const fn const_u64_to_usize(value: u64) -> usize {
    if size_of::<usize>() >= size_of::<u64>() {
        return value as usize;
    }
    if value > usize::MAX as u64 {
        panic!();
    }
    value as usize
}

pub const fn const_size_of_u32<T>() -> u32 {
    const_usize_to_u32(size_of::<T>())
}

pub const fn const_size_of_u64<T>() -> u64 {
    const_usize_to_u64(size_of::<T>())
}

pub const fn const_size_of_value_u64<T>(_: &T) -> u64 {
    const_size_of_u64::<T>()
}

pub const fn const_max_u32_slice(s: &[u32]) -> u32 {
    let mut max = 0;
    let mut i = 0;

    while i < s.len() {
        if s[i] > max {
            max = s[i];
        }

        i += 1;
    }

    max
}
