use std::{error, fmt};

use crate::{IMMEDIATES_SIZE, TAGGED_IMAGE_UNIT_SIZE, U32_SIZE, Vec2U32};

pub use crate::config::errors::*;

#[derive(Clone, Copy)]
pub struct OversizedImmediatesError {
    pub(crate) max_immediate_size: u32,
}
impl OversizedImmediatesError {
    #[inline]
    pub fn device_max_immediate_size(&self) -> u32 {
        self.max_immediate_size
    }

    #[inline]
    pub fn required_immediate_size(&self) -> u32 {
        IMMEDIATES_SIZE
    }
}
impl fmt::Debug for OversizedImmediatesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OversizedImmediatesError")
            .field(
                "device_limits",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("wgpu::Limits")
                        .field("max_immediate_size", &self.max_immediate_size)
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "required",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("immediate_size", &IMMEDIATES_SIZE)
                        .finish_non_exhaustive()
                }),
            )
            .finish()
    }
}
impl fmt::Display for OversizedImmediatesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "library requires immediate size of {IMMEDIATES_SIZE} bytes, wgpu device setup with maximum of {} bytes",
            self.max_immediate_size,
        )
    }
}
impl error::Error for OversizedImmediatesError {}

#[derive(Clone, Copy)]
pub struct SizeOverflowError {
    pub(crate) max_pixels: u64,
}
impl SizeOverflowError {
    #[inline]
    pub fn requested_max_pixels(&self) -> u64 {
        self.max_pixels
    }
}
impl fmt::Debug for SizeOverflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SizeOverflowError")
            .field("requested_max_pixels", &self.max_pixels)
            .finish()
    }
}
impl fmt::Display for SizeOverflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "size of largest buffer overflows u64 with {} requested maximum pixels ({} × {TAGGED_IMAGE_UNIT_SIZE} > u64::MAX)",
            self.max_pixels, self.max_pixels,
        )
    }
}
impl error::Error for SizeOverflowError {}

#[derive(Clone, Copy)]
pub struct OversizedBufferError {
    pub(crate) size: u64,
    pub(crate) max_buffer_size: u64,
    pub(crate) max_storage_buffer_binding_size: u64,
}
impl OversizedBufferError {
    #[inline]
    pub fn device_max_buffer_size(&self) -> u64 {
        self.max_buffer_size
    }

    #[inline]
    pub fn device_max_storage_buffer_binding_size(&self) -> u64 {
        self.max_storage_buffer_binding_size
    }

    #[inline]
    pub fn requested_max_buffer_size(&self) -> u64 {
        self.size
    }

    #[inline]
    pub fn device_size_limit(&self) -> u64 {
        self.max_buffer_size
            .min(self.max_storage_buffer_binding_size)
    }

    #[inline]
    pub fn device_max_pixels(&self) -> u64 {
        self.device_size_limit() / TAGGED_IMAGE_UNIT_SIZE
    }

    #[inline]
    pub fn requested_max_pixels(&self) -> u64 {
        self.size / TAGGED_IMAGE_UNIT_SIZE
    }
}
impl fmt::Debug for OversizedBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OversizedBufferError")
            .field(
                "device_limits",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("wgpu::Limits")
                        .field("max_buffer_size", &self.max_buffer_size)
                        .field(
                            "max_storage_buffer_binding_size",
                            &self.max_storage_buffer_binding_size,
                        )
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "requested",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("buffer_size", &self.size)
                        .field("storage_buffer_binding_size", &self.size)
                        .finish_non_exhaustive()
                }),
            )
            .finish()
    }
}
impl fmt::Display for OversizedBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "size of largest requested buffer ({} bytes, {} pixels) is larger than at least one of the device's maximum buffer size ({} bytes) and the device's maximum storage buffer binding size ({} bytes)",
            self.size,
            self.requested_max_pixels(),
            self.max_buffer_size,
            self.max_storage_buffer_binding_size,
        )
    }
}
impl error::Error for OversizedBufferError {}

#[derive(Clone, Copy)]
pub enum NewError {
    OversizedImmediates(OversizedImmediatesError),
    SizeOverflow(SizeOverflowError),
    OversizedBuffer(OversizedBufferError),
}
impl NewError {
    pub const fn source(&self) -> &(dyn error::Error + 'static) {
        match self {
            NewError::OversizedImmediates(err) => err,
            NewError::SizeOverflow(err) => err,
            NewError::OversizedBuffer(err) => err,
        }
    }
}
impl fmt::Debug for NewError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.source(), f)
    }
}
impl fmt::Display for NewError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.source(), f)
    }
}
impl error::Error for NewError {
    #[inline]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(self.source())
    }
}

impl From<OversizedImmediatesError> for NewError {
    #[inline]
    fn from(value: OversizedImmediatesError) -> Self {
        Self::OversizedImmediates(value)
    }
}
impl From<SizeOverflowError> for NewError {
    #[inline]
    fn from(value: SizeOverflowError) -> Self {
        Self::SizeOverflow(value)
    }
}
impl From<OversizedBufferError> for NewError {
    #[inline]
    fn from(value: OversizedBufferError) -> Self {
        Self::OversizedBuffer(value)
    }
}

#[derive(Clone, Copy)]
pub struct OversizedImageError {
    pub(crate) max_pixels: u64,
    pub(crate) pixel_size: Vec2U32,
}
impl OversizedImageError {
    #[inline(always)]
    pub(crate) fn check(pixel_size: Vec2U32, max_pixels: u64) -> Result<u64, Self> {
        let pixels = pixel_size.product();
        if pixels > max_pixels {
            Err(Self {
                max_pixels,
                pixel_size,
            })
        } else {
            Ok(pixels)
        }
    }

    #[inline]
    pub fn max_pixels(&self) -> u64 {
        self.max_pixels
    }

    #[inline]
    pub fn requested_pixel_size(&self) -> Vec2U32 {
        self.pixel_size
    }

    #[inline]
    pub fn requested_pixels(&self) -> u64 {
        self.pixel_size.product()
    }
}
impl fmt::Debug for OversizedImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OversizedImageError")
            .field(
                "limits",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("max_pixels", &self.max_pixels)
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "requested",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("pixel_size", &self.pixel_size)
                        .field("pixels", &self.pixel_size.product())
                        .finish_non_exhaustive()
                }),
            )
            .finish()
    }
}
impl fmt::Display for OversizedImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "requested image size ({} × {} = {} pixels) is larger than the maximum set at creation ({} pixels)",
            self.pixel_size.x,
            self.pixel_size.y,
            self.pixel_size.product(),
            self.max_pixels,
        )
    }
}
impl error::Error for OversizedImageError {}

#[derive(Clone, Copy)]
pub struct UndersizedBufferError {
    buffer_size: u64,
    pixel_size: Vec2U32,
}
impl UndersizedBufferError {
    #[inline]
    pub fn buffer_size(&self) -> u64 {
        self.buffer_size
    }

    #[inline]
    pub fn requested_pixel_size(&self) -> Vec2U32 {
        self.pixel_size
    }

    #[inline]
    pub fn buffer_max_pixels(&self) -> u64 {
        self.buffer_size / U32_SIZE
    }

    #[inline]
    pub fn requested_pixels(&self) -> u64 {
        self.pixel_size.product()
    }

    #[inline]
    pub fn requested_size(&self) -> u64 {
        self.requested_pixels() * U32_SIZE.get()
    }
}
impl fmt::Debug for UndersizedBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UndersizedBufferError")
            .field(
                "limits",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("buffer_size")
                        .field("", &self.buffer_size)
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "requested",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("pixel_size", &self.pixel_size)
                        .field("pixels", &self.pixel_size.product())
                        .field("size", &self.requested_size())
                        .finish_non_exhaustive()
                }),
            )
            .finish()
    }
}
impl fmt::Display for UndersizedBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "provided buffer of size {} bytes is too small for requested image size {} × {} ({} pixels, {} bytes)",
            self.buffer_size,
            self.pixel_size.x,
            self.pixel_size.y,
            self.pixel_size.product(),
            self.requested_size(),
        )
    }
}
impl error::Error for UndersizedBufferError {}

#[derive(Clone, Copy)]
pub enum CopyError {
    OversizedImage(OversizedImageError),
    UndersizedBuffer(UndersizedBufferError),
}
impl CopyError {
    pub const fn source(&self) -> &(dyn error::Error + 'static) {
        match self {
            CopyError::OversizedImage(err) => err,
            CopyError::UndersizedBuffer(err) => err,
        }
    }

    #[inline(always)]
    pub(crate) fn check(
        pixel_size: Vec2U32,
        max_pixels: u64,
        buffer_size: u64,
    ) -> Result<u64, Self> {
        let pixels = OversizedImageError::check(pixel_size, max_pixels)?;

        let size = pixels * U32_SIZE.get();
        if size > buffer_size {
            Err(UndersizedBufferError {
                buffer_size,
                pixel_size,
            }
            .into())
        } else {
            Ok(size)
        }
    }
}
impl fmt::Debug for CopyError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.source(), f)
    }
}
impl fmt::Display for CopyError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.source(), f)
    }
}
impl error::Error for CopyError {
    #[inline]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(self.source())
    }
}

impl From<OversizedImageError> for CopyError {
    #[inline]
    fn from(value: OversizedImageError) -> Self {
        Self::OversizedImage(value)
    }
}
impl From<UndersizedBufferError> for CopyError {
    #[inline]
    fn from(value: UndersizedBufferError) -> Self {
        Self::UndersizedBuffer(value)
    }
}
