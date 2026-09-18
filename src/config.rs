use std::{mem, num::NonZero};

use crate::{
    config::errors::{AtCapacityError, NotFoundError, OutOfOrderError, PushSortedError},
    utils::const_size_of_u64,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Sort {
    #[default]
    None = 0b00,
    Increasing = 0b10,
    Decreasing = 0b11,
}
impl Sort {
    pub const DEFAULT: Self = Self::None;

    #[inline]
    pub const fn get(self) -> u32 {
        self as u8 as u32
    }
}

// TODO: default
#[derive(Clone)]
pub struct ConfigBuffer {
    len: NonZero<u8>,
    boundaries: [(u8, Sort); ConfigBuffer::CAPACITY + 1],
}

// TODO:
// - compress len + sort into one vec2,
// - or pad boundaries with u8::MAX and remove len,
// - or put len at start of boundaries?
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Config {
    pub boundaries: [u8; ConfigBuffer::CAPACITY + 1],
    pub sort: u32,
    pub _padding: [u32; 3],
}

pub const CONFIG_SIZE: NonZero<u64> = NonZero::new(const_size_of_u64::<Config>()).unwrap();
const ONE: NonZero<u8> = NonZero::new(1).unwrap();

impl ConfigBuffer {
    pub const CAPACITY_U8: u8 = 16 - 1;
    pub const CAPACITY: usize = Self::CAPACITY_U8 as usize;

    #[inline]
    pub const fn new() -> Self {
        Self {
            len: ONE,
            boundaries: [(0, Sort::DEFAULT); Self::CAPACITY + 1],
        }
    }

    #[inline]
    pub const fn len(&self) -> NonZero<u8> {
        self.len
    }

    #[inline]
    pub const fn finish(&self) -> Config {
        let mut boundaries = [0; Self::CAPACITY + 1];
        let mut sort = 0;

        let mut i = 0;
        while i < self.len.get() {
            let (key, value) = self.boundaries[i as usize];

            if let Some(j) = i.checked_sub(1) {
                assert!(key != 0);
                boundaries[j as usize] = key;
            }
            sort |= value.get() << (i * 2);

            i += 1;
        }

        Config {
            boundaries,
            sort,
            _padding: [0; 3],
        }
    }

    #[inline]
    pub const fn clear(&mut self) {
        self.boundaries[0] = (0, Sort::None);
        self.len = ONE;
    }

    #[inline]
    pub fn boundaries(&self) -> &[(u8, Sort)] {
        &self.boundaries[..self.len.get().into()]
    }

    pub const fn push_sorted(&mut self, boundary: u8, sort: Sort) -> Result<(), PushSortedError> {
        let last_i = self.len.get() - 1;
        let last = self.boundaries[last_i as usize].0;
        let i = if boundary > last {
            if self.len.get() >= Self::CAPACITY_U8 {
                return Err(PushSortedError::AtCapacity(AtCapacityError {}));
            }

            let new_len = self.len.saturating_add(1);
            mem::replace(&mut self.len, new_len).get()
        } else if boundary == last {
            last_i
        } else {
            return Err(PushSortedError::OutOfOrder(OutOfOrderError {
                last_boundary: last,
                boundary,
            }));
        };

        self.boundaries[i as usize] = (boundary, sort);

        Ok(())
    }

    pub const fn pop_highest(&mut self) -> (u8, Sort) {
        let Some(i) = NonZero::new(self.len.get() - 1) else {
            return mem::replace(&mut self.boundaries[0], (0, Sort::DEFAULT));
        };
        self.len = i;

        self.boundaries[i.get() as usize]
    }

    pub fn insert(&mut self, boundary: u8, sort: Sort) -> Result<(), AtCapacityError> {
        let i = self
            .boundaries()
            .binary_search_by_key(&boundary, |(key, _value)| *key)
            .or_else(|i| {
                if self.len.get() >= Self::CAPACITY_U8 {
                    Err(AtCapacityError {})
                } else {
                    self.boundaries.copy_within(i..self.len.get().into(), i + 1);
                    self.len = self.len.saturating_add(1);
                    Ok(i)
                }
            })?;

        self.boundaries[i] = (boundary, sort);

        Ok(())
    }

    pub fn get(self, boundary: u8) -> Result<Sort, NotFoundError> {
        self.boundaries()
            .binary_search_by_key(&boundary, |(key, _value)| *key)
            .map(|i| self.boundaries[i].1)
            .map_err(|_| NotFoundError { boundary })
    }

    pub fn remove(&mut self, boundary: u8) -> Result<Sort, NotFoundError> {
        if boundary == 0 {
            let (_, value) = mem::replace(&mut self.boundaries[0], (0, Sort::DEFAULT));
            return Ok(value);
        }

        let i = self
            .boundaries()
            .binary_search_by_key(&boundary, |(key, _value)| *key)
            .map_err(|_| NotFoundError { boundary })?;

        let (_, sort) = self.boundaries[i];
        self.boundaries
            .copy_within((i + 1)..self.len.get().into(), i);

        Ok(sort)
    }

    #[inline]
    pub const fn single_sorted(before: Sort, boundary: u8, after: Sort) -> Self {
        let mut config = Self::new();
        debug_assert!(config.push_sorted(0, before).is_ok());
        debug_assert!(config.push_sorted(boundary, after).is_ok());
        config
    }
}

impl Default for ConfigBuffer {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl From<ConfigBuffer> for Config {
    #[inline]
    fn from(value: ConfigBuffer) -> Self {
        value.finish()
    }
}

pub mod errors {
    use std::{error, fmt};

    use crate::config::ConfigBuffer;

    #[derive(Clone, Copy)]
    pub struct AtCapacityError {}
    impl AtCapacityError {
        #[inline]
        pub const fn capacity(&self) -> u8 {
            ConfigBuffer::CAPACITY_U8
        }
    }
    impl fmt::Debug for AtCapacityError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("AtCapacityError")
                .field("capacity", &ConfigBuffer::CAPACITY_U8)
                .finish()
        }
    }
    impl fmt::Display for AtCapacityError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "cannot push to full config buffer (length = capacity = {})",
                ConfigBuffer::CAPACITY_U8,
            )
        }
    }
    impl error::Error for AtCapacityError {}

    #[derive(Clone, Copy)]
    pub struct OutOfOrderError {
        pub(crate) last_boundary: u8,
        pub(crate) boundary: u8,
    }
    impl OutOfOrderError {
        #[inline]
        pub fn last_boundary(&self) -> u8 {
            self.last_boundary
        }

        #[inline]
        pub fn requested_boundary(&self) -> u8 {
            self.boundary
        }
    }
    impl fmt::Debug for OutOfOrderError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("OutOfOrderError")
                .field("last_boundary", &self.last_boundary)
                .field("requested_boundary", &self.boundary)
                .finish()
        }
    }
    impl fmt::Display for OutOfOrderError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "cannot push to config buffer out of order (attempted to push boundary {}, last boundary {})",
                self.boundary, self.last_boundary,
            )
        }
    }
    impl error::Error for OutOfOrderError {}

    #[derive(Clone, Copy)]
    pub enum PushSortedError {
        AtCapacity(AtCapacityError),
        OutOfOrder(OutOfOrderError),
    }
    impl PushSortedError {
        pub const fn source(&self) -> &(dyn error::Error + 'static) {
            match self {
                PushSortedError::AtCapacity(err) => err,
                PushSortedError::OutOfOrder(err) => err,
            }
        }
    }
    impl fmt::Debug for PushSortedError {
        #[inline]
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Debug::fmt(self.source(), f)
        }
    }
    impl fmt::Display for PushSortedError {
        #[inline]
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Display::fmt(self.source(), f)
        }
    }
    impl error::Error for PushSortedError {
        #[inline]
        fn source(&self) -> Option<&(dyn error::Error + 'static)> {
            Some(self.source())
        }
    }

    #[derive(Clone, Copy)]
    pub struct NotFoundError {
        pub(crate) boundary: u8,
    }
    impl NotFoundError {
        #[inline]
        pub const fn requested_boundary(&self) -> u8 {
            self.boundary
        }
    }
    impl fmt::Debug for NotFoundError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("NotFoundError")
                .field("requested_boundary", &self.boundary)
                .finish()
        }
    }
    impl fmt::Display for NotFoundError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "requested boundary ({}) not found in config buffer",
                self.boundary,
            )
        }
    }
    impl error::Error for NotFoundError {}
}

#[cfg(test)]
mod test {
    use crate::config::{ConfigBuffer, Sort};

    #[test]
    fn single_sorted() {
        let buffer = ConfigBuffer::single_sorted(Sort::Increasing, 128, Sort::Decreasing);
        assert_eq!(buffer.len.get(), 2);
        assert_eq!(
            buffer.boundaries(),
            &[(0, Sort::Increasing), (128, Sort::Decreasing)]
        );

        let config = buffer.finish();
        assert_eq!(&config.boundaries[..2], &[128, 0]);
        assert_eq!(
            config.sort,
            Sort::Increasing.get() | Sort::Decreasing.get() << 2,
        );
    }
}
