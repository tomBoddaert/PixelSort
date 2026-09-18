use std::{error, fmt, num::NonZero};

use crate::{IMMEDIATES_SIZE, TAGGED_IMAGE_UNIT_SIZE, U32_SIZE, Vec2U32, config::CONFIG_SIZE};

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
                        .field("immediate_size", &self.required_immediate_size())
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
            "library requires immediate size of {} bytes, wgpu device setup with maximum of {} bytes",
            self.required_immediate_size(),
            self.max_immediate_size,
        )
    }
}
impl error::Error for OversizedImmediatesError {}

#[derive(Clone, Copy)]
pub struct SizeOverflowError {
    pub(crate) max_pixels: NonZero<u64>,
}
impl SizeOverflowError {
    #[inline]
    pub fn requested_max_pixels(&self) -> NonZero<u64> {
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

#[derive(Clone, Copy, Debug)]
pub enum SizeSource {
    Image { pixels: NonZero<u64> },
    Config,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OversizedBufferLimit {
    MaxBufferSize,
    MaxUniformBufferBindingSize,
    MaxStorageBufferBindingSize,
}
impl OversizedBufferLimit {
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MaxBufferSize => "max_buffer_size",
            Self::MaxUniformBufferBindingSize => "max_uniform_buffer_binding_size",
            Self::MaxStorageBufferBindingSize => "max_storage_buffer_binding_size",
        }
    }

    #[inline]
    const fn dbg_requested_name(self) -> &'static str {
        match self {
            Self::MaxBufferSize => "buffer_size",
            Self::MaxUniformBufferBindingSize => "uniform_buffer_binding_size",
            Self::MaxStorageBufferBindingSize => "storage_buffer_binding_size",
        }
    }

    #[inline]
    pub const fn get(self, limits: &wgpu::Limits) -> u64 {
        match self {
            OversizedBufferLimit::MaxBufferSize => limits.max_buffer_size,
            OversizedBufferLimit::MaxUniformBufferBindingSize => {
                limits.max_uniform_buffer_binding_size
            }
            OversizedBufferLimit::MaxStorageBufferBindingSize => {
                limits.max_storage_buffer_binding_size
            }
        }
    }

    #[inline]
    pub(crate) const fn create(self, limits: &wgpu::Limits) -> (Self, u64) {
        (self, self.get(limits))
    }
}
#[derive(Clone, Copy)]
pub struct OversizedBufferError {
    pub(crate) source: SizeSource,
    pub(crate) size: u64,
    pub(crate) limit: (OversizedBufferLimit, u64),
}
impl OversizedBufferError {
    #[inline]
    pub(crate) const fn check(
        source: SizeSource,
        limit: (OversizedBufferLimit, u64),
        size: u64,
    ) -> Result<(), Self> {
        if size > limit.1 {
            Err(Self {
                source,
                size,
                limit,
            })
        } else {
            Ok(())
        }
    }

    #[inline]
    pub const fn source(&self) -> SizeSource {
        self.source
    }

    #[inline]
    pub const fn device_limit(&self) -> (OversizedBufferLimit, u64) {
        self.limit
    }

    #[inline]
    pub const fn requested_buffer_size(&self) -> u64 {
        self.size
    }
}
impl fmt::Debug for OversizedBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (limit, limit_value) = self.limit;

        f.debug_struct("OversizedBufferError")
            .field(
                "device_limits",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("wgpu::Limits")
                        .field(limit.name(), &limit_value)
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "requested",
                &fmt::from_fn(|fmt| {
                    let mut str = fmt.debug_struct("");
                    if let SizeSource::Image { pixels } = self.source {
                        str.field("max_pixels", &pixels);
                    }
                    str.field(limit.dbg_requested_name(), &self.size)
                        .finish_non_exhaustive()
                }),
            )
            .finish()
    }
}
impl fmt::Display for OversizedBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (limit, limit_value) = self.limit;

        match self.source {
            SizeSource::Image { pixels: max_pixels } => write!(
                f,
                "{} max pixels requires buffer size of {} but the device limit {} is only {}",
                max_pixels,
                self.size,
                limit.name(),
                limit_value,
            ),
            SizeSource::Config => write!(
                f,
                "config buffer requires size of {} but the device limit {} is only {}",
                self.size,
                limit.name(),
                limit_value,
            ),
        }
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
    pub const fn max_pixels(&self) -> u64 {
        self.max_pixels
    }

    #[inline]
    pub const fn requested_pixel_size(&self) -> Vec2U32 {
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
pub struct UndersizedImageBufferError {
    buffer_size: u64,
    pixel_size: Vec2U32,
}
impl UndersizedImageBufferError {
    #[inline]
    pub const fn buffer_size(&self) -> u64 {
        self.buffer_size
    }

    #[inline]
    pub const fn requested_pixel_size(&self) -> Vec2U32 {
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
impl fmt::Debug for UndersizedImageBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UndersizedBufferError")
            .field(
                "required",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("buffer_size")
                        .field("", &self.buffer_size)
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "provided",
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
impl fmt::Display for UndersizedImageBufferError {
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
impl error::Error for UndersizedImageBufferError {}

#[derive(Clone, Copy)]
pub enum CopyImageError {
    OversizedImage(OversizedImageError),
    UndersizedImageBuffer(UndersizedImageBufferError),
}
impl CopyImageError {
    pub const fn source(&self) -> &(dyn error::Error + 'static) {
        match self {
            CopyImageError::OversizedImage(err) => err,
            CopyImageError::UndersizedImageBuffer(err) => err,
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
            Err(UndersizedImageBufferError {
                buffer_size,
                pixel_size,
            }
            .into())
        } else {
            Ok(size)
        }
    }
}
impl fmt::Debug for CopyImageError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.source(), f)
    }
}
impl fmt::Display for CopyImageError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.source(), f)
    }
}
impl error::Error for CopyImageError {
    #[inline]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(self.source())
    }
}

impl From<OversizedImageError> for CopyImageError {
    #[inline]
    fn from(value: OversizedImageError) -> Self {
        Self::OversizedImage(value)
    }
}
impl From<UndersizedImageBufferError> for CopyImageError {
    #[inline]
    fn from(value: UndersizedImageBufferError) -> Self {
        Self::UndersizedImageBuffer(value)
    }
}

#[derive(Clone, Copy)]
pub struct UndersizedConfigBufferError {
    buffer_size: u64,
    offset: u32,
}
impl UndersizedConfigBufferError {
    pub(crate) fn check(buffer_size: u64, offset: u32) -> Result<u64, Self> {
        let byte_offset = u64::from(offset) * CONFIG_SIZE.get();
        if buffer_size < byte_offset + CONFIG_SIZE.get() {
            Err(Self {
                buffer_size,
                offset,
            })
        } else {
            Ok(byte_offset)
        }
    }

    #[inline]
    pub const fn buffer_size(&self) -> u64 {
        self.buffer_size
    }

    #[inline]
    pub const fn config_offset(&self) -> u32 {
        self.offset
    }

    #[inline]
    pub fn required_size(&self) -> u64 {
        (u64::from(self.offset) + 1) * CONFIG_SIZE.get()
    }
}
impl fmt::Debug for UndersizedConfigBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UndersizedConfigBufferError")
            .field(
                "required",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("buffer_size", &self.required_size())
                        .finish_non_exhaustive()
                }),
            )
            .field(
                "provided",
                &fmt::from_fn(|fmt| {
                    fmt.debug_struct("")
                        .field("buffer_size", &self.buffer_size)
                        .finish_non_exhaustive()
                }),
            )
            .finish()
    }
}
impl fmt::Display for UndersizedConfigBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "provided buffer of size {} bytes is too small for config of size {} bytes at offset of {} bytes ({} bytes total)",
            self.buffer_size,
            CONFIG_SIZE,
            u64::from(self.offset) * CONFIG_SIZE.get(),
            self.required_size(),
        )
    }
}
impl error::Error for UndersizedConfigBufferError {}
