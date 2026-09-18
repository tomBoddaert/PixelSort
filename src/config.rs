use std::num::NonZero;

use crate::{
    errors::{AtCapacityError, EmptyError, NotFoundError, OutOfOrderError, PushSortedError},
    utils::const_size_of_u64,
};

#[derive(Clone, Default)]
pub struct ConfigBuffer {
    len: u8,
    boundaries: [u8; ConfigBuffer::CAPACITY + 1],
    sort: u32,
}

// TODO:
// - compress len + sort into one vec2,
// - or pad boundaries with u8::MAX and remove len,
// - or put len at start of boundaries?
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Config {
    pub len: u32,
    pub _padding1: [u32; 3],
    pub boundaries: [u8; ConfigBuffer::CAPACITY + 1],
    pub sort: u32,
    pub _padding2: [u32; 3],
}

pub const CONFIG_SIZE: NonZero<u64> = NonZero::new(const_size_of_u64::<Config>()).unwrap();

impl ConfigBuffer {
    pub const CAPACITY_U8: u8 = 32 - 1;
    pub const CAPACITY: usize = Self::CAPACITY_U8 as usize;

    #[inline]
    pub const fn new() -> Self {
        Self {
            len: 0,
            boundaries: [0; Self::CAPACITY + 1],
            sort: 0,
        }
    }

    #[inline]
    pub const fn len(&self) -> u8 {
        self.len
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub const fn finish(&self) -> Config {
        Config {
            len: self.len as u32,
            _padding1: [0; 3],
            boundaries: self.boundaries,
            sort: self.sort,
            _padding2: [0; 3],
        }
    }

    #[inline]
    pub const fn clear(&mut self) {
        self.len = 0;
    }

    #[inline]
    pub fn boundaries(&self) -> &[u8] {
        &self.boundaries[..self.len.into()]
    }

    #[inline]
    const fn set_sort(&mut self, i: u8, sort: bool) {
        let mask = !(1 << i);
        let flag = (sort as u32) << i;
        self.sort = (self.sort & mask) | flag;
    }

    #[inline]
    const fn get_sort(&self, i: u8) -> bool {
        let mask = 1 << i;
        self.sort & mask != 0
    }

    pub const fn push_sorted(&mut self, boundary: u8, sort: bool) -> Result<(), PushSortedError> {
        if boundary == 0 {
            self.set_sort(0, sort);
            return Ok(());
        }

        let (i, new) = if let Some(last_i) = self.len.checked_sub(1) {
            let last = self.boundaries[last_i as usize];
            if boundary > last {
                if self.len == Self::CAPACITY_U8 {
                    return Err(PushSortedError::AtCapacity(AtCapacityError {}));
                }
                (self.len, true)
            } else if boundary == last {
                (last_i, false)
            } else {
                return Err(PushSortedError::OutOfOrder(OutOfOrderError {
                    last_boundary: last,
                    boundary,
                }));
            }
        } else {
            (0, true)
        };

        if new {
            self.boundaries[i as usize] = boundary;
            self.len += 1;
        }
        self.set_sort(i + 1, sort);

        Ok(())
    }

    pub const fn pop_highest(&mut self) -> Result<(u8, bool), EmptyError> {
        let i = match self.len.checked_sub(1) {
            Some(i) => i,
            None => return Err(EmptyError {}),
        };

        let boundary = self.boundaries[i as usize];
        let sort = self.get_sort(i + 1);

        self.len = i;
        Ok((boundary, sort))
    }

    pub fn insert(&mut self, boundary: u8, sort: bool) -> Result<(), AtCapacityError> {
        if boundary == 0 {
            self.set_sort(0, sort);
            return Ok(());
        }

        let i = match self.boundaries().binary_search(&boundary) {
            Ok(i) => i as u8,
            Err(i) => {
                if self.len >= Self::CAPACITY_U8 {
                    return Err(AtCapacityError {});
                }

                self.boundaries.copy_within(i..self.len.into(), i + 1);
                self.boundaries[i] = boundary;

                let i = i as u8;

                let lower_mask = (1 << (i + 1)) - 1;
                self.sort = (self.sort & !lower_mask) << 1 | (self.sort & lower_mask);

                i
            }
        };

        self.set_sort(i + 1, sort);

        Ok(())
    }

    pub fn is_sorted(self, boundary: u8) -> Result<bool, NotFoundError> {
        let i = match self.boundaries().binary_search(&boundary) {
            Ok(i) => i,
            Err(_) => return Err(NotFoundError { boundary }),
        };

        Ok(self.get_sort(i as u8 + 1))
    }

    pub fn remove(&mut self, boundary: u8) -> Result<bool, NotFoundError> {
        let i = match self.boundaries().binary_search(&boundary) {
            Ok(i) => i,
            Err(_) => return Err(NotFoundError { boundary }),
        };

        let sort = self.get_sort(i as u8 + 1);

        self.boundaries.copy_within((i + 1)..self.len.into(), i);

        let lower_mask = (1 << (i + 1)) - 1;
        self.sort = (self.sort & (!lower_mask << 1)) >> 1 | (self.sort & lower_mask);

        Ok(sort)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, bool)> {
        std::iter::once(0).chain(1..=self.len).map(|i| {
            (
                i.checked_sub(1)
                    .map_or(0, |j| self.boundaries[usize::from(j)]),
                self.get_sort(i),
            )
        })
    }

    #[inline]
    pub const fn single_sorted(before: bool, boundary: u8, after: bool) -> Self {
        let mut config = Self::new();
        debug_assert!(config.push_sorted(0, before).is_ok());
        debug_assert!(config.push_sorted(boundary, after).is_ok());
        config
    }
}

impl From<ConfigBuffer> for Config {
    #[inline]
    fn from(value: ConfigBuffer) -> Self {
        value.finish()
    }
}

pub(crate) mod errors {
    use std::{error, fmt};

    use crate::ConfigBuffer;

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
    pub struct EmptyError {}
    impl fmt::Debug for EmptyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("EmptyError").finish()
        }
    }
    impl fmt::Display for EmptyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "cannot pop from empty config buffer")
        }
    }
    impl error::Error for EmptyError {}

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
