use std::num::NonZero;

use crate::{
    errors::{
        CopyImageError, NewError, OversizedBufferError, OversizedBufferLimit, OversizedImageError,
        OversizedImmediatesError, SizeOverflowError, UndersizedConfigBufferError,
    },
    utils::{U32_SIZE, U64_SIZE, const_size_of_u32},
};

mod config;
pub mod errors;
pub mod utils;
pub use config::*;

pub const BIT_LEN: u32 = 4;
pub const BASE: u32 = 2_u32.pow(BIT_LEN);
pub const SHADER_MODULE_DESCRIPTOR: wgpu::ShaderModuleDescriptor =
    wgpu::include_wgsl!("pixel_sort.wgsl");
const TAGGED_IMAGE_UNIT_SIZE: u64 = U64_SIZE.get() * 2;
pub const REQUIRED_FEATURES: wgpu::Features = wgpu::Features::IMMEDIATES;
pub const IMMEDIATES_SIZE: u32 = const_size_of_u32::<Immediates>();

#[cfg(not(test))]
const TEST_COPY_SRC: wgpu::BufferUsages = wgpu::BufferUsages::empty();
#[cfg(test)]
const TEST_COPY_SRC: wgpu::BufferUsages = wgpu::BufferUsages::COPY_SRC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Binding {
    Image = 0,
    Config = 1,
    TaggedImage = 2,
    Output = 3,
}
impl Binding {
    #[inline]
    pub const fn get(self) -> u32 {
        self as u32
    }
}
impl From<Binding> for u32 {
    #[inline]
    fn from(value: Binding) -> Self {
        value.get()
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Vec2U32 {
    pub x: u32,
    pub y: u32,
}
impl Vec2U32 {
    #[must_use]
    #[inline]
    pub fn product(self) -> u64 {
        u64::from(self.x) * u64::from(self.y)
    }
}

pub struct PixelSort {
    pub max_pixels: u64,
    pub input: wgpu::Buffer,
    pub config: wgpu::Buffer,
    pub tagged_image: wgpu::Buffer,
    pub output: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_size: u32,
}

impl PixelSort {
    // TODO: remove NonZero?
    // TODO: lower overflow checks to u32, as GPU indexing is done with u32, not u64
    pub fn new(
        device: &wgpu::Device,
        workgroup_size: u32,
        max_pixels: NonZero<u64>,
    ) -> Result<Self, NewError> {
        let limits = device.limits();

        // TODO: check device features

        if IMMEDIATES_SIZE > limits.max_immediate_size {
            return Err(OversizedImmediatesError {
                max_immediate_size: limits.max_immediate_size,
            }
            .into());
        }

        // Largest buffer size needed
        let tagged_image_size = max_pixels
            .get()
            .checked_mul(TAGGED_IMAGE_UNIT_SIZE)
            .ok_or(SizeOverflowError { max_pixels })?;

        let max_size = tagged_image_size > CONFIG_SIZE.get();
        OversizedBufferError::check(
            if max_size {
                errors::SizeSource::Image { pixels: max_pixels }
            } else {
                errors::SizeSource::Config
            },
            OversizedBufferLimit::MaxBufferSize.create(&limits),
            if max_size {
                tagged_image_size
            } else {
                CONFIG_SIZE.get()
            },
        )?;
        OversizedBufferError::check(
            errors::SizeSource::Image { pixels: max_pixels },
            OversizedBufferLimit::MaxStorageBufferBindingSize.create(&limits),
            tagged_image_size,
        )?;
        OversizedBufferError::check(
            errors::SizeSource::Config,
            OversizedBufferLimit::MaxUniformBufferBindingSize.create(&limits),
            CONFIG_SIZE.get(),
        )?;

        let module = device.create_shader_module(SHADER_MODULE_DESCRIPTOR);

        let input = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PixelSort::input"),
            size: max_pixels.get() * U32_SIZE.get(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let input_layout = wgpu::BindGroupLayoutEntry {
            binding: Binding::Image.get(),
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let input_entry = wgpu::BindGroupEntry {
            binding: input_layout.binding,
            resource: input.as_entire_binding(),
        };

        let config = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PixelSort::config"),
            size: CONFIG_SIZE.get(),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let config_layout = wgpu::BindGroupLayoutEntry {
            binding: Binding::Config.get(),
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(CONFIG_SIZE),
            },
            count: None,
        };
        let config_entry = wgpu::BindGroupEntry {
            binding: config_layout.binding,
            resource: config.as_entire_binding(),
        };

        let tagged_image = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PixelSort::tagged_image"),
            size: tagged_image_size,
            usage: wgpu::BufferUsages::STORAGE | TEST_COPY_SRC,
            mapped_at_creation: false,
        });
        let tagged_image_layout = wgpu::BindGroupLayoutEntry {
            binding: Binding::TaggedImage.get(),
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: Some(U64_SIZE),
            },
            count: None,
        };
        let tagged_image_entry = wgpu::BindGroupEntry {
            binding: tagged_image_layout.binding,
            resource: tagged_image.as_entire_binding(),
        };

        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PixelSort::output"),
            size: input.size(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let output_layout = wgpu::BindGroupLayoutEntry {
            binding: Binding::Output.get(),
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let output_entry = wgpu::BindGroupEntry {
            binding: output_layout.binding,
            resource: output.as_entire_binding(),
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PixelSort bind_group_layout"),
            entries: &[
                input_layout,
                config_layout,
                tagged_image_layout,
                output_layout,
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PixelSort::bind_group"),
            layout: &bind_group_layout,
            entries: &[input_entry, config_entry, tagged_image_entry, output_entry],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("PixelSort pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: const { const_size_of_u32::<Immediates>() },
        });
        let compilation_constants = [("workgroup_size", workgroup_size.into())];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("PixelSort::pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &compilation_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        Ok(Self {
            max_pixels: max_pixels.get(),
            input,
            config,
            tagged_image,
            output,
            bind_group,
            pipeline,
            workgroup_size,
        })
    }

    // TODO: zero check
    pub fn copy_to_input(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image: &wgpu::Buffer,
        image_size: Vec2U32,
    ) -> Result<(), CopyImageError> {
        let size = CopyImageError::check(image_size, self.max_pixels, image.size())?;
        encoder.copy_buffer_to_buffer(image, 0, &self.input, 0, size);
        Ok(())
    }

    pub fn copy_to_config(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        configs: &wgpu::Buffer,
        offset: u32,
    ) -> Result<(), UndersizedConfigBufferError> {
        let byte_offset = UndersizedConfigBufferError::check(configs.size(), offset)?;
        encoder.copy_buffer_to_buffer(configs, byte_offset, &self.config, 0, CONFIG_SIZE.get());
        Ok(())
    }

    // TODO: zero check
    pub fn copy_from_output(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::Buffer,
        image_size: Vec2U32,
    ) -> Result<(), CopyImageError> {
        let size = CopyImageError::check(image_size, self.max_pixels, output.size())?;
        encoder.copy_buffer_to_buffer(&self.output, 0, output, 0, size);
        Ok(())
    }

    // TODO: zero check
    pub fn copy_output_to_input(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image_size: Vec2U32,
    ) -> Result<(), OversizedImageError> {
        let pixels = OversizedImageError::check(image_size, self.max_pixels)?;
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.input, 0, pixels * U32_SIZE.get());
        Ok(())
    }

    // TODO: zero check
    pub fn add_step(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image_size: Vec2U32,
        vertical: bool,
    ) -> Result<(), OversizedImageError> {
        OversizedImageError::check(image_size, self.max_pixels)?;

        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("PixelSort compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let width = if vertical { image_size.y } else { image_size.x };
        let block_size = width.div_ceil(self.workgroup_size);
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                width,
                block_size,
                vertical: vertical.into(),
            }),
        );

        compute_pass.dispatch_workgroups(1, if vertical { image_size.x } else { image_size.y }, 1);

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct BoolU32(pub u32);
impl From<bool> for BoolU32 {
    #[inline]
    fn from(value: bool) -> Self {
        Self(value.into())
    }
}
impl From<BoolU32> for bool {
    #[inline]
    fn from(value: BoolU32) -> Self {
        value.0 != 0
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub width: u32,
    pub block_size: u32,
    pub vertical: BoolU32,
}

#[inline]
pub const fn required_storage_buffer_binding_size(
    max_pixels: NonZero<u64>,
) -> Result<u64, SizeOverflowError> {
    if let Some(size) = max_pixels.get().checked_mul(TAGGED_IMAGE_UNIT_SIZE) {
        Ok(size)
    } else {
        Err(SizeOverflowError { max_pixels })
    }
}
#[inline]
pub const fn required_uniform_buffer_binding_size() -> u64 {
    CONFIG_SIZE.get()
}
pub fn add_requred_features_and_limits(
    mut device_descriptor: wgpu::DeviceDescriptor,
    max_pixels: NonZero<u64>,
) -> Result<wgpu::DeviceDescriptor, SizeOverflowError> {
    let storage_size = required_storage_buffer_binding_size(max_pixels)?;
    let uniform_size = required_uniform_buffer_binding_size();

    device_descriptor.required_features |= REQUIRED_FEATURES;

    device_descriptor.required_limits.max_immediate_size = device_descriptor
        .required_limits
        .max_immediate_size
        .max(IMMEDIATES_SIZE);
    device_descriptor.required_limits.max_buffer_size = device_descriptor
        .required_limits
        .max_buffer_size
        .max(storage_size)
        .max(uniform_size);
    device_descriptor
        .required_limits
        .max_storage_buffer_binding_size = device_descriptor
        .required_limits
        .max_storage_buffer_binding_size
        .max(storage_size);
    device_descriptor
        .required_limits
        .max_uniform_buffer_binding_size = device_descriptor
        .required_limits
        .max_uniform_buffer_binding_size
        .max(uniform_size);

    Ok(device_descriptor)
}

#[cfg(test)]
mod test {
    use std::{num::NonZero, sync::OnceLock};

    use wgpu::util::DeviceExt;

    use crate::{
        BASE, Binding, CONFIG_SIZE, Config, ConfigBuffer, IMMEDIATES_SIZE, Immediates, PixelSort,
        SHADER_MODULE_DESCRIPTOR, Vec2U32,
        utils::{
            const_size_of_u32, const_size_of_u64, const_size_of_value_u64, const_u32_to_usize,
            const_u64_to_usize,
        },
    };

    struct State {
        pub device: wgpu::Device,
        pub queue: wgpu::Queue,
        pub module: wgpu::ShaderModule,
    }
    static STATE: OnceLock<State> = OnceLock::new();
    fn get_state() -> &'static State {
        STATE.get_or_init(|| {
            // env_logger::init();

            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

            let adapter = pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
            )
            .unwrap();
            let downlevel_capabilities = adapter.get_downlevel_capabilities();
            if !downlevel_capabilities
                .flags
                .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
            {
                panic!("Adapter does not support compute shaders");
            }

            let mut required_limits = wgpu::Limits::defaults();
            required_limits.max_immediate_size = IMMEDIATES_SIZE;

            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::IMMEDIATES | wgpu::Features::SUBGROUP,
                    required_limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                }))
                .expect("Failed to create device");

            let module = device.create_shader_module(SHADER_MODULE_DESCRIPTOR);

            State {
                device,
                queue,
                module,
            }
        })
    }

    const WORKGROUP_SIZE: u32 = 5;
    const WORKGROUP_SIZE_USIZE: usize = const_u32_to_usize(WORKGROUP_SIZE);
    const COMPILATION_CONSTANTS: [(&str, f64); 1] = [("workgroup_size", WORKGROUP_SIZE as f64)];

    const IMAGE_SIZE: Vec2U32 = Vec2U32 { x: 18, y: 3 };
    const IMAGE_WIDTH_USIZE: usize = const_u32_to_usize(IMAGE_SIZE.x);
    const IMAGE_HEIGHT_USIZE: usize = const_u32_to_usize(IMAGE_SIZE.y);
    const IMAGE_PIXELS: NonZero<u64> =
        NonZero::new(IMAGE_SIZE.x as u64 * IMAGE_SIZE.y as u64).unwrap();
    const IMAGE_PIXELS_USIZE: usize = const_u64_to_usize(IMAGE_PIXELS.get());
    const IMAGE_VALUE: [[u8; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = [
        [
            0x26, 0x49, 0x17, 0x6a, /**/ 0x40, 0x2c, 0x0d, 0x20, /**/ 0x7e, 0x21, 0x4d,
            0x68, /**/ 0x3b, 0x40, 0x39, 0x2d, /**/ 0x5b, 0x6a,
        ],
        [
            0x3e, 0xad, 0x49, 0x70, /**/ 0x05, 0x24, 0x5e, 0x5d, /**/ 0x66, 0xdb, 0xd3,
            0xdc, /**/ 0x0d, 0x39, 0x62, 0x14, /**/ 0xcd, 0x6f,
        ],
        [
            0xa4, 0x75, 0xe4, 0x76, /**/ 0xbe, 0x4d, 0xd9, 0x5e, /**/ 0x95, 0x0b, 0xfd,
            0x69, /**/ 0xec, 0x23, 0xe3, 0x1a, /**/ 0xc6, 0x38,
        ],
    ];
    const IMAGE: [[u32; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = {
        let mut image = [[0; _]; _];

        let mut i = 0;
        while i < IMAGE_PIXELS_USIZE {
            let value = IMAGE_VALUE.as_flattened()[i] as u32;
            image.as_flattened_mut()[i] = 0x010101 * value;

            i += 1;
        }

        image
    };
    const CONFIG: Config = ConfigBuffer::single_sorted(true, 128, true).finish();
    const BLOCK_SIZE: u32 = IMAGE_SIZE.x.div_ceil(WORKGROUP_SIZE);
    const _BOUNDED: [[u8; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = [
        [0; IMAGE_WIDTH_USIZE],
        [
            0, 1, 0, 0, /**/ 0, 0, 0, 0, /**/ 0, 1, 1, 1, /**/ 0, 0, 0, 0,
            /**/ 1, 0,
        ],
        [
            1, 0, 1, 0, /**/ 1, 0, 1, 0, /**/ 1, 0, 1, 0, /**/ 1, 0, 1, 0,
            /**/ 1, 0,
        ],
    ];
    const PREVIOUS_BLOCK_REGION_COUNT: [[u32; WORKGROUP_SIZE_USIZE]; IMAGE_HEIGHT_USIZE] =
        [[0; 5], [0, 2, 0, 2, 1], [0, 4, 4, 4, 4]];
    const PREVIOUS_BLOCK_TAG: [[u32; WORKGROUP_SIZE_USIZE]; IMAGE_HEIGHT_USIZE] =
        [[0; 5], [0, 2, 2, 4, 5], [0, 4, 8, 12, 16]];
    const IMAGE_TAG: [[u32; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = [
        [0; IMAGE_WIDTH_USIZE],
        [
            0, 1, 2, 2, /**/ 2, 2, 2, 2, /**/ 2, 3, 3, 3, /**/ 4, 4, 4, 4,
            /**/ 5, 6,
        ],
        [
            0, 1, 2, 3, /**/ 4, 5, 6, 7, /**/ 8, 9, 10, 11, /**/ 12, 13, 14, 15,
            /**/ 16, 17,
        ],
    ];
    // [IMAGE_TAG.0 << 16 | IMAGE_VALUE.0, IMAGE.0], [0, 0], [IMAGE_TAG.1 << 16 | IMAGE_VALUE.1, IMAGE.1], [0, 0], ...
    const TAGGED_IMAGE: [[[u32; 2]; 2 * IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = {
        let mut tagged_image = [[[0; 2]; _]; _];

        let mut i = 0;
        while i < IMAGE_PIXELS_USIZE {
            let rgb = IMAGE.as_flattened()[i];
            let value = IMAGE_VALUE.as_flattened()[i] as u32;
            let tag = IMAGE_TAG.as_flattened()[i];
            tagged_image.as_flattened_mut()[2 * i] = [tag << 16 | value, rgb];

            i += 1;
        }

        tagged_image
    };
    const TAG_P_COUNT: [[[u32; const_u32_to_usize(BASE)]; WORKGROUP_SIZE_USIZE];
        IMAGE_HEIGHT_USIZE] = [
        [
            [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
        [
            [1, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
        [
            [1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1],
            [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
    ];
    const TAG_COUNT_PARTIAL_SUM: [[[u32; const_u32_to_usize(BASE)]; WORKGROUP_SIZE_USIZE];
        IMAGE_HEIGHT_USIZE] = [
        [
            [
                0, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
            ],
            [
                4, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
            ],
            [
                8, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
            ],
            [
                12, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
            ],
            [
                16, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
            ],
        ],
        [
            [0, 1, 2, 9, 12, 16, 17, 18, 18, 18, 18, 18, 18, 18, 18, 18],
            [1, 2, 4, 9, 12, 16, 17, 18, 18, 18, 18, 18, 18, 18, 18, 18],
            [1, 2, 8, 9, 12, 16, 17, 18, 18, 18, 18, 18, 18, 18, 18, 18],
            [1, 2, 9, 12, 12, 16, 17, 18, 18, 18, 18, 18, 18, 18, 18, 18],
            [1, 2, 9, 12, 16, 16, 17, 18, 18, 18, 18, 18, 18, 18, 18, 18],
        ],
        [
            [0, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
            [1, 3, 5, 6, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
            [1, 3, 5, 6, 7, 8, 9, 10, 10, 11, 12, 13, 14, 15, 16, 17],
            [1, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 14, 15, 16, 17],
            [1, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
        ],
    ];
    const _VALUE_P_COUNT: [[[u32; const_u32_to_usize(BASE)]; WORKGROUP_SIZE_USIZE];
        IMAGE_HEIGHT_USIZE] = [
        [
            //0 1  2  3  4  5  6  7  8  9  a  b  c  d  e  f
            [0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0],
            [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0],
            [0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0],
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0],
        ],
        [
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0],
            [0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0],
            [0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0],
            [0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
        ],
        [
            [0, 0, 0, 0, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 2, 0],
            [0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0],
            [0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        ],
    ];
    const VALUE_COUNT_PARTIAL_SUM: [[[u32; const_u32_to_usize(BASE)]; WORKGROUP_SIZE_USIZE];
        IMAGE_HEIGHT_USIZE] = [
        [
            //0 1  2  3  4  5  6  7  8  9  a  b   c   d   e   f
            [0, 3, 4, 4, 4, 4, 4, 5, 6, 7, 9, 11, 13, 14, 17, 18],
            [0, 3, 4, 4, 4, 4, 5, 6, 6, 8, 10, 11, 13, 14, 17, 18],
            [2, 3, 4, 4, 4, 4, 5, 6, 6, 8, 10, 11, 14, 15, 17, 18],
            [2, 4, 4, 4, 4, 4, 5, 6, 7, 8, 10, 11, 14, 16, 18, 18],
            [3, 4, 4, 4, 4, 4, 5, 6, 7, 9, 10, 12, 14, 17, 18, 18],
        ],
        [
            //0 1  2  3  4  5  6  7  8  9  a  b  c   d   e   f
            [0, 1, 1, 2, 3, 5, 6, 7, 7, 7, 9, 9, 10, 11, 15, 17],
            [1, 1, 1, 2, 3, 5, 6, 7, 7, 8, 9, 9, 10, 12, 16, 17],
            [1, 1, 1, 2, 4, 6, 6, 7, 7, 8, 9, 9, 10, 13, 17, 17],
            [1, 1, 1, 3, 4, 6, 7, 7, 7, 8, 9, 10, 11, 13, 17, 17],
            [1, 1, 2, 3, 5, 6, 7, 7, 7, 9, 9, 10, 11, 14, 17, 17],
        ],
        [
            //0 1  2  3  4  5  6  7  8  9  a   b   c   d   e   f
            [0, 0, 0, 0, 2, 4, 6, 8, 8, 9, 11, 12, 13, 14, 16, 18],
            [0, 0, 0, 0, 4, 5, 7, 8, 8, 9, 11, 12, 13, 14, 16, 18],
            [0, 0, 0, 0, 4, 5, 7, 8, 8, 10, 11, 12, 13, 15, 18, 18],
            [0, 0, 0, 0, 4, 6, 7, 8, 8, 11, 11, 13, 13, 16, 18, 18],
            [0, 0, 0, 2, 4, 6, 7, 8, 8, 11, 12, 13, 14, 16, 18, 18],
        ],
    ];
    const IMAGE_REORDER: [[u32; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = [
        [
            4, 7, 5, 9, /**/ 0, 13, 14, 1, /**/ 17, 3, 15, 6, /**/ 11, 2, 8, 16,
            /**/ 12, 10,
        ],
        [
            15, 11, 7, 0, /**/ 5, 3, 16, 12, /**/ 6, 9, 2, 10, /**/ 13, 8, 1, 4,
            /**/ 14, 17,
        ],
        [
            2, 4, 3, 6, /**/ 16, 14, 9, 17, /**/ 5, 12, 15, 10, /**/ 13, 0, 1, 11,
            /**/ 7, 8,
        ],
    ];
    const TAGGED_IMAGE_REORDERED: [[[u32; 2]; 2 * IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = {
        let mut tagged_image = TAGGED_IMAGE;

        let mut y = 0;
        while y < IMAGE_HEIGHT_USIZE {
            let mut x = 0;
            while x < IMAGE_WIDTH_USIZE {
                let tagged_pixel = tagged_image[y][x << 1];
                let pos = IMAGE_REORDER[y][x];
                tagged_image[y][const_u32_to_usize(pos) << 1 | 1] = tagged_pixel;

                x += 1;
            }

            y += 1;
        }

        tagged_image
    };

    #[derive(Clone, Copy, Default)]
    enum BufferSetup<T, Size = u64> {
        #[default]
        None,
        _Uninitialised(Size),
        UninitialisedFrom(T),
        Initialised(T),
    }
    struct Buffer {
        buffer: wgpu::Buffer,
        layout: wgpu::BindGroupLayoutEntry,
    }
    impl Buffer {
        fn create<E: bytemuck::Pod>(
            setup: BufferSetup<&'_ [E]>,
            device: &wgpu::Device,
            label: &'static str,
            binding: u32,
            read_only: bool,
        ) -> Self {
            let t_size = const { const_size_of_u64::<E>() };
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;

            let buffer = match setup {
                BufferSetup::None => device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: t_size,
                    usage,
                    mapped_at_creation: false,
                }),
                BufferSetup::_Uninitialised(size) => {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size,
                        usage,
                        mapped_at_creation: false,
                    })
                }
                BufferSetup::UninitialisedFrom(value) => {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size: u64::try_from(value.len())
                            .unwrap()
                            .checked_mul(t_size)
                            .unwrap(),
                        usage,
                        mapped_at_creation: false,
                    })
                }
                BufferSetup::Initialised(value) => {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytemuck::cast_slice(value),
                        usage,
                    })
                }
            };
            let layout = wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only },
                    has_dynamic_offset: false,
                    min_binding_size: Some(NonZero::new(t_size).unwrap()),
                },
                count: None,
            };

            Self { buffer, layout }
        }

        fn entry(&self) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding: self.layout.binding,
                resource: self.buffer.as_entire_binding(),
            }
        }
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum DownloadSource {
        _Output,
        TaggedImage,
        TestBuffer,
    }
    #[derive(Default)]
    struct Setup<Input, TaggedImage, Output, OutputSize, TestBuffer> {
        input: BufferSetup<Input>,
        tagged_image: BufferSetup<TaggedImage>,
        output: BufferSetup<Output, OutputSize>,
        test_buffer: BufferSetup<TestBuffer>,
    }
    struct Instance {
        state: &'static State,
        _input: Buffer,
        _config: Buffer,
        tagged_image: Buffer,
        output: Buffer,
        bind_group: wgpu::BindGroup,
        test_buffer: Buffer,
        test_bind_group: wgpu::BindGroup,
        pipeline: wgpu::ComputePipeline,
        download_source: DownloadSource,
        download: wgpu::Buffer,
    }
    fn setup(
        options: Setup<
            Option<&'_ [[u32; IMAGE_WIDTH_USIZE]]>,
            Option<&'_ [[[u32; 2]; 2 * IMAGE_WIDTH_USIZE]]>,
            &'_ [[u32; IMAGE_WIDTH_USIZE]],
            Option<u64>,
            &'_ [u32],
        >,
        entry_point: &str,
        download_source: DownloadSource,
    ) -> Instance {
        let state = get_state();
        let State { device, module, .. } = state;

        let Setup {
            input,
            tagged_image,
            output,
            test_buffer,
        } = options;
        let options = Setup {
            input: match input {
                BufferSetup::None => BufferSetup::None,
                BufferSetup::_Uninitialised(size) => BufferSetup::_Uninitialised(size),
                BufferSetup::UninitialisedFrom(value) => {
                    BufferSetup::UninitialisedFrom(value.unwrap_or(&IMAGE))
                }
                BufferSetup::Initialised(value) => {
                    BufferSetup::Initialised(value.unwrap_or(&IMAGE))
                }
            },
            tagged_image: match tagged_image {
                BufferSetup::None => BufferSetup::None,
                BufferSetup::_Uninitialised(size) => BufferSetup::_Uninitialised(size),
                BufferSetup::UninitialisedFrom(value) => {
                    BufferSetup::UninitialisedFrom(value.unwrap_or(&TAGGED_IMAGE))
                }
                BufferSetup::Initialised(value) => {
                    BufferSetup::Initialised(value.unwrap_or(&TAGGED_IMAGE))
                }
            },
            output: match output {
                BufferSetup::None => BufferSetup::None,
                BufferSetup::_Uninitialised(size) => {
                    BufferSetup::_Uninitialised(size.unwrap_or(const_size_of_value_u64(&IMAGE)))
                }
                BufferSetup::UninitialisedFrom(value) => BufferSetup::UninitialisedFrom(value),
                BufferSetup::Initialised(value) => BufferSetup::Initialised(value),
            },
            test_buffer,
        };

        let input = Buffer::create(options.input, device, "input", Binding::Image.get(), true);
        let config = {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("config"),
                contents: bytemuck::bytes_of(&CONFIG),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let layout = wgpu::BindGroupLayoutEntry {
                binding: Binding::Config.get(),
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(CONFIG_SIZE),
                },
                count: None,
            };

            Buffer { buffer, layout }
        };
        let tagged_image = Buffer::create(
            options.tagged_image,
            device,
            "tagged_image",
            Binding::TaggedImage.get(),
            false,
        );
        let output = Buffer::create(
            options.input,
            device,
            "output",
            Binding::Output.get(),
            false,
        );

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bind_group_layout"),
            entries: &[
                input.layout,
                tagged_image.layout,
                config.layout,
                output.layout,
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind_group"),
            layout: &bind_group_layout,
            entries: &[
                input.entry(),
                tagged_image.entry(),
                config.entry(),
                output.entry(),
            ],
        });

        let test_buffer = Buffer::create(options.test_buffer, device, "test_buffer", 0, false);

        let test_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bind_group_layout"),
                entries: &[test_buffer.layout],
            });
        let test_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind_group"),
            layout: &test_bind_group_layout,
            entries: &[test_buffer.entry()],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind_group_layout), Some(&test_bind_group_layout)],
            immediate_size: const { const_size_of_u32::<Immediates>() },
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module,
            entry_point: Some(entry_point),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &COMPILATION_CONSTANTS,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("download"),
            size: match download_source {
                DownloadSource::_Output => &output,
                DownloadSource::TaggedImage => &tagged_image,
                DownloadSource::TestBuffer => &test_buffer,
            }
            .buffer
            .size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Instance {
            state,
            _input: input,
            _config: config,
            tagged_image,
            output,
            bind_group,
            test_buffer,
            test_bind_group,
            pipeline,
            download_source,
            download,
        }
    }
    impl Instance {
        fn submit(&self) -> wgpu::BufferView {
            let mut encoder =
                self.state
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("encoder"),
                    });

            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("compute_pass"),
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &self.bind_group, &[]);
            compute_pass.set_bind_group(1, &self.test_bind_group, &[]);
            compute_pass.set_immediates(
                0,
                bytemuck::bytes_of(&Immediates {
                    width: IMAGE_SIZE.x,
                    block_size: BLOCK_SIZE,
                    vertical: false.into(),
                }),
            );
            compute_pass.dispatch_workgroups(1, IMAGE_SIZE.y, 1);
            drop(compute_pass);

            let download_source = &match self.download_source {
                DownloadSource::_Output => &self.output,
                DownloadSource::TaggedImage => &self.tagged_image,
                DownloadSource::TestBuffer => &self.test_buffer,
            }
            .buffer;
            encoder.copy_buffer_to_buffer(
                download_source,
                0,
                &self.download,
                0,
                download_source.size(),
            );
            encoder.map_buffer_on_submit(&self.download, wgpu::MapMode::Read, .., |_| {});

            let ix = self.state.queue.submit([encoder.finish()]);
            self.state
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(ix),
                    timeout: None,
                })
                .unwrap();

            self.download.get_mapped_range(..).unwrap()
        }
    }

    #[test]
    fn region_count() {
        let instance = setup(
            Setup {
                input: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::UninitialisedFrom(
                    PREVIOUS_BLOCK_REGION_COUNT.as_flattened(),
                ),
                ..Setup::default()
            },
            "test_region_count",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [u32; _]>(&downloaded);

        assert_eq!(result, PREVIOUS_BLOCK_REGION_COUNT);
    }

    #[test]
    fn wg_partial_sum() {
        let instance = setup(
            Setup {
                input: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::Initialised(PREVIOUS_BLOCK_REGION_COUNT.as_flattened()),
                ..Setup::default()
            },
            "test_wg_partial_sum",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [u32; _]>(&downloaded);

        assert_eq!(result, PREVIOUS_BLOCK_TAG);
    }

    #[test]
    fn label_regions() {
        let instance = setup(
            Setup {
                input: BufferSetup::Initialised(None),
                tagged_image: BufferSetup::UninitialisedFrom(None),
                test_buffer: BufferSetup::Initialised(PREVIOUS_BLOCK_TAG.as_flattened()),
                ..Setup::default()
            },
            "test_label_regions",
            DownloadSource::TaggedImage,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; 2]; _]>(&downloaded);

        assert_eq!(result, TAGGED_IMAGE);
    }

    #[test]
    fn p_count() {
        let instance = setup(
            Setup {
                tagged_image: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::UninitialisedFrom(bytemuck::cast_slice(&TAG_P_COUNT)),
                ..Setup::default()
            },
            "test_p_count",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; _]; _]>(&downloaded);

        assert_eq!(result, TAG_P_COUNT);
    }

    #[test]
    fn wg_partial_sum_counts() {
        let instance = setup(
            Setup {
                test_buffer: BufferSetup::Initialised(bytemuck::cast_slice(&TAG_P_COUNT)),
                ..Setup::default()
            },
            "test_wg_partial_sum_counts",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; _]; _]>(&downloaded);

        assert_eq!(result, TAG_COUNT_PARTIAL_SUM);
    }

    #[test]
    fn reorder() {
        let instance = setup(
            Setup {
                tagged_image: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::Initialised(bytemuck::cast_slice(
                    &VALUE_COUNT_PARTIAL_SUM,
                )),
                ..Setup::default()
            },
            "test_reorder",
            DownloadSource::TaggedImage,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; _]; _]>(&downloaded);

        assert_eq!(result, TAGGED_IMAGE_REORDERED);
    }

    #[test]
    fn partial_sort() {
        let instance = setup(
            Setup {
                tagged_image: BufferSetup::Initialised(None),
                ..Setup::default()
            },
            "test_partial_sort",
            DownloadSource::TaggedImage,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; _]; _]>(&downloaded);

        assert_eq!(result, TAGGED_IMAGE_REORDERED);
    }

    #[test]
    fn partial_sort2() {
        let mut reordered = [[[0; 2]; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE];
        for (pos, pixel) in TAGGED_IMAGE_REORDERED
            .as_flattened()
            .iter()
            .skip(1)
            .step_by(2)
            .enumerate()
        {
            reordered.as_flattened_mut()[pos] = *pixel;
        }
        let expected = reordered.map(|mut row| {
            row.sort_by_key(|[tag, _rgb]| tag & 0xf0000);
            row
        });

        let instance = setup(
            Setup {
                tagged_image: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::UninitialisedFrom(bytemuck::cast_slice(&expected)),
                ..Setup::default()
            },
            "test_partial_sort2",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; 2]; IMAGE_WIDTH_USIZE]>(&downloaded);

        assert_eq!(result, expected);
    }

    #[test]
    fn sort() {
        let mut tagged = [[[0; 2]; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE];
        for (pos, pixel) in TAGGED_IMAGE.as_flattened().iter().step_by(2).enumerate() {
            tagged.as_flattened_mut()[pos] = *pixel;
        }
        let expected = tagged.map(|mut row| {
            row.sort_by_key(|[tag, _rgb]| *tag);
            row
        });

        let instance = setup(
            Setup {
                tagged_image: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::UninitialisedFrom(bytemuck::cast_slice(&expected)),
                ..Setup::default()
            },
            "test_sort",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [[u32; 2]; IMAGE_WIDTH_USIZE]>(&downloaded);

        assert_eq!(result, expected);
    }

    #[test]
    fn full() {
        let State { device, queue, .. } = get_state();

        let pixel_sort = PixelSort::new(device, WORKGROUP_SIZE, IMAGE_PIXELS).unwrap();

        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&IMAGE),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let config = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&CONFIG),
            usage: image.usage(),
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: pixel_sort
                .tagged_image
                .size()
                .checked_add(pixel_sort.input.size())
                .unwrap(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        pixel_sort
            .copy_to_input(&mut encoder, &image, IMAGE_SIZE)
            .unwrap();
        pixel_sort.copy_to_config(&mut encoder, &config, 0).unwrap();
        pixel_sort
            .add_step(&mut encoder, IMAGE_SIZE, false)
            .unwrap();
        encoder.copy_buffer_to_buffer(
            &pixel_sort.tagged_image,
            0,
            &download,
            0,
            pixel_sort.tagged_image.size(),
        );
        encoder.copy_buffer_to_buffer(
            &pixel_sort.output,
            0,
            &download,
            pixel_sort.tagged_image.size(),
            pixel_sort.input.size(),
        );
        encoder.map_buffer_on_submit(&download, wgpu::MapMode::Read, .., |_| {});
        let ix = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(ix),
                timeout: None,
            })
            .unwrap();

        let tagged_image_bytes = download
            .get_mapped_range(..pixel_sort.tagged_image.size())
            .unwrap();
        let tagged_image = bytemuck::from_bytes::<
            [[[u32; 2]; 2 * IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE],
        >(&tagged_image_bytes);
        let active_tagged_image = tagged_image.map(|row| {
            let mut new = [[0; 2]; IMAGE_WIDTH_USIZE];
            row.iter()
                .step_by(2)
                .zip(&mut new)
                .for_each(|(a, b)| *b = *a);
            new
        });

        let image_bytes = download
            .get_mapped_range(pixel_sort.tagged_image.size()..)
            .unwrap();
        let image =
            bytemuck::from_bytes::<[[u32; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE]>(&image_bytes);

        let mut tagged = [[[0; 2]; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE];
        for (pos, pixel) in TAGGED_IMAGE.as_flattened().iter().step_by(2).enumerate() {
            tagged.as_flattened_mut()[pos] = *pixel;
        }
        let expected = tagged.map(|mut row| {
            row.sort_by_key(|[tag, _rgb]| *tag);
            row
        });
        let expected_image = expected.map(|row| row.map(|[_tag, rgb]| rgb));

        assert_eq!(active_tagged_image, expected);
        assert_eq!(image, &expected_image);
    }
}
