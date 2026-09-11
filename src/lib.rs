use std::num::NonZero;

pub const U32_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u32>())).unwrap();
pub const U64_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u64>())).unwrap();
pub const BIT_LEN: u32 = 4;
pub const BASE: u32 = 2_u32.pow(BIT_LEN);

// TODO: replace this with a wgsl module to copy buffers without COPY_SRC for testing
#[cfg(not(test))]
const TEST_COPY_SRC: wgpu::BufferUsages = wgpu::BufferUsages::empty();
#[cfg(test)]
const TEST_COPY_SRC: wgpu::BufferUsages = wgpu::BufferUsages::COPY_SRC;

pub struct PixelSort {
    pub max_pixels: u64,
    pub input: wgpu::Buffer,
    pub tagged_image: wgpu::Buffer,
    pub output: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_size: u32,
}

impl PixelSort {
    pub fn new(device: &wgpu::Device, workgroup_size: u32, max_pixels: u64) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("pixel_sort.wgsl"));

        let input = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PixelSort::input"),
            size: max_pixels.checked_mul(U32_SIZE.get()).unwrap(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let input_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
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

        let tagged_image = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PixelSort::tagged_image"),
            size: max_pixels.checked_mul(U64_SIZE.get() * 2).unwrap(),
            usage: wgpu::BufferUsages::STORAGE | TEST_COPY_SRC,
            mapped_at_creation: false,
        });
        let tagged_image_layout = wgpu::BindGroupLayoutEntry {
            binding: 1,
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
            binding: 2,
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
            entries: &[input_layout, tagged_image_layout, output_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PixelSort::bind_group"),
            layout: &bind_group_layout,
            entries: &[input_entry, tagged_image_entry, output_entry],
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

        Self {
            max_pixels,
            input,
            tagged_image,
            output,
            bind_group,
            pipeline,
            workgroup_size,
        }
    }

    pub fn copy_to_input(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image: &wgpu::Buffer,
        image_size: Vec2U32,
    ) {
        let pixels = image_size.product();
        assert!(pixels <= self.max_pixels);
        encoder.copy_buffer_to_buffer(image, 0, &self.input, 0, pixels * U32_SIZE.get());
    }

    pub fn copy_from_output(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::Buffer,
        image_size: Vec2U32,
    ) {
        let pixels = image_size.product();
        assert!(pixels <= self.max_pixels);
        encoder.copy_buffer_to_buffer(&self.output, 0, output, 0, pixels * U32_SIZE.get());
    }

    pub fn add_step(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image_size: Vec2U32,
        threshold: f32,
    ) {
        let pixels = image_size.product();
        assert!(pixels <= self.max_pixels);

        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("PixelSort compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let block_size = image_size.x.div_ceil(self.workgroup_size);
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                width: image_size.x,
                block_size,
                threshold,
            }),
        );

        compute_pass.dispatch_workgroups(1, image_size.y, 1);
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub width: u32,
    pub block_size: u32,
    pub threshold: f32,
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

pub const IMMEDIATES_SIZE: u32 = const_size_of_u32::<Immediates>();

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

#[cfg(test)]
mod test {
    use std::{num::NonZero, sync::OnceLock};

    use wgpu::util::DeviceExt;

    use crate::{
        BASE, IMMEDIATES_SIZE, Immediates, PixelSort, Vec2U32, const_size_of_u32,
        const_size_of_u64, const_size_of_value_u64, const_u32_to_usize, const_u64_to_usize,
    };

    struct State {
        pub device: wgpu::Device,
        pub queue: wgpu::Queue,
        pub module: wgpu::ShaderModule,
    }
    static STATE: OnceLock<State> = OnceLock::new();
    fn get_state() -> &'static State {
        STATE.get_or_init(|| {
            env_logger::init();

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

            let module = device.create_shader_module(wgpu::include_wgsl!("pixel_sort.wgsl"));

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
    const IMAGE_PIXELS: u64 = IMAGE_SIZE.x as u64 * IMAGE_SIZE.y as u64;
    const IMAGE_PIXELS_USIZE: usize = const_u64_to_usize(IMAGE_PIXELS);
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
    const THRESHOLD: f32 = 0.5;
    const BLOCK_SIZE: u32 = IMAGE_SIZE.x.div_ceil(WORKGROUP_SIZE);
    const THRESHOLDED: [[bool; IMAGE_WIDTH_USIZE]; IMAGE_HEIGHT_USIZE] = [
        [false; IMAGE_WIDTH_USIZE],
        [
            false, true, false, false, /**/ false, false, false, false, /**/ false, true,
            true, true, /**/ false, false, false, false, /**/ true, false,
        ],
        [
            true, false, true, false, /**/ true, false, true, false, /**/ true, false,
            true, false, /**/ true, false, true, false, /**/ true, false,
        ],
    ];
    const PREVIOUS_BLOCK_CHANGE_COUNT: [[u32; WORKGROUP_SIZE_USIZE]; IMAGE_HEIGHT_USIZE] =
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
    const VALUE_P_COUNT: [[[u32; const_u32_to_usize(BASE)]; WORKGROUP_SIZE_USIZE];
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
        Uninitialised(Size),
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
                BufferSetup::Uninitialised(size) => device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage,
                    mapped_at_creation: false,
                }),
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
        Output,
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
        input: Buffer,
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
                BufferSetup::Uninitialised(size) => BufferSetup::Uninitialised(size),
                BufferSetup::UninitialisedFrom(value) => {
                    BufferSetup::UninitialisedFrom(value.unwrap_or(&IMAGE))
                }
                BufferSetup::Initialised(value) => {
                    BufferSetup::Initialised(value.unwrap_or(&IMAGE))
                }
            },
            tagged_image: match tagged_image {
                BufferSetup::None => BufferSetup::None,
                BufferSetup::Uninitialised(size) => BufferSetup::Uninitialised(size),
                BufferSetup::UninitialisedFrom(value) => {
                    BufferSetup::UninitialisedFrom(value.unwrap_or(&TAGGED_IMAGE))
                }
                BufferSetup::Initialised(value) => {
                    BufferSetup::Initialised(value.unwrap_or(&TAGGED_IMAGE))
                }
            },
            output: match output {
                BufferSetup::None => BufferSetup::None,
                BufferSetup::Uninitialised(size) => {
                    BufferSetup::Uninitialised(size.unwrap_or(const_size_of_value_u64(&IMAGE)))
                }
                BufferSetup::UninitialisedFrom(value) => BufferSetup::UninitialisedFrom(value),
                BufferSetup::Initialised(value) => BufferSetup::Initialised(value),
            },
            test_buffer,
        };

        let input = Buffer::create(options.input, device, "input", 0, true);
        let tagged_image = Buffer::create(options.tagged_image, device, "tagged_image", 1, false);
        let output = Buffer::create(options.input, device, "output", 2, false);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bind_group_layout"),
            entries: &[input.layout, tagged_image.layout, output.layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind_group"),
            layout: &bind_group_layout,
            entries: &[input.entry(), tagged_image.entry(), output.entry()],
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
                DownloadSource::Output => &output,
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
            input,
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
                    threshold: THRESHOLD,
                }),
            );
            compute_pass.dispatch_workgroups(1, IMAGE_SIZE.y, 1);
            drop(compute_pass);

            let download_source = &match self.download_source {
                DownloadSource::Output => &self.output,
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
    fn change_count() {
        let instance = setup(
            Setup {
                input: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::UninitialisedFrom(
                    PREVIOUS_BLOCK_CHANGE_COUNT.as_flattened(),
                ),
                ..Setup::default()
            },
            "test_change_count",
            DownloadSource::TestBuffer,
        );

        let downloaded = instance.submit();
        let result = bytemuck::cast_slice::<u8, [u32; _]>(&downloaded);

        assert_eq!(result, PREVIOUS_BLOCK_CHANGE_COUNT);
    }

    #[test]
    fn wg_partial_sum() {
        let instance = setup(
            Setup {
                input: BufferSetup::Initialised(None),
                test_buffer: BufferSetup::UninitialisedFrom(PREVIOUS_BLOCK_TAG.as_flattened()),
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

        let pixel_sort = PixelSort::new(device, WORKGROUP_SIZE, IMAGE_PIXELS);

        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&IMAGE),
            usage: wgpu::BufferUsages::COPY_SRC,
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
        pixel_sort.copy_to_input(&mut encoder, &image, IMAGE_SIZE);
        pixel_sort.add_step(&mut encoder, IMAGE_SIZE, THRESHOLD);
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
