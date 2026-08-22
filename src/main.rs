use std::num::NonZero;

use wgpu::util::DeviceExt;

const U32_SIZE: NonZero<u64> = NonZero::new(4).unwrap();
const BIT_LEN: u32 = 4;
const BASE: u32 = 2_u32.pow(BIT_LEN);

fn main() {
    env_logger::init();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let downlevel_capabilities = adapter.get_downlevel_capabilities();
    if !downlevel_capabilities
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    {
        panic!("Adapter does not support compute shaders");
    }

    let mut required_limits = wgpu::Limits::defaults();
    required_limits.max_immediate_size = 16;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::IMMEDIATES,
        required_limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
    }))
    .expect("Failed to create device");

    let workgroup_info = WorkgroupInfo {
        workgroup_size: adapter.get_info().subgroup_max_size,
        max_workgroups: 32,
    };

    let test_values: [u32; _] = [
        38, 10, 34, 67, 27, 16, 63, 36, 29, 68, 19, 67, 16, 94, 39, 42, 14, 4, 13, 15, 11, 60, 11,
        72, 0, 82, 49, 80, 26, 93, 11, 77, 70, 92, 49, 26, 72, 55, 25, 8, 30, 75, 49, 53, 67, 15,
        38, 21, 11, 37, 53, 65, 41, 34, 41, 73, 24, 15, 73, 38, 97, 70, 39, 93, 5, 91, 88, 72, 25,
        98, 79, 11, 53, 54, 38, 26, 58, 10, 35, 37, 55, 82, 33, 58, 57, 14, 49, 65, 90, 98, 48, 66,
        56, 76, 77, 67, 5, 53, 75, 3,
    ];
    let test_values_len = test_values.len().try_into().unwrap();
    let sort = Sort::new(&device, workgroup_info, test_values_len);

    let upload = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("upload"),
        contents: bytemuck::cast_slice(&test_values),
        usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
    });
    let download = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("download"),
        size: upload.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("encoder"),
    });

    encoder.copy_buffer_to_buffer(&upload, 0, &sort.count.input, 0, upload.size());

    sort.add_steps(&mut encoder, test_values_len);

    encoder.copy_buffer_to_buffer(&sort.reorder.output, 0, &download, 0, download.size());
    encoder.map_buffer_on_submit(&download, wgpu::MapMode::Read, .., |_| {});

    let ix = queue.submit([encoder.finish()]);
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(ix),
            timeout: None,
        })
        .unwrap();

    let download_mapped = download.get_mapped_range(..).unwrap();
    let downloaded = bytemuck::cast_slice::<u8, u32>(&download_mapped);
    println!("Input:\t{test_values:>2?}");
    println!("Output:\t{downloaded:>2?}");

    let mut correct = test_values;
    correct.sort_unstable();
    assert_eq!(downloaded, &correct);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WorkgroupInfo {
    workgroup_size: u32,
    max_workgroups: u32,
}

struct Count {
    input: wgpu::Buffer,
    counts: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    workgroup_info: WorkgroupInfo,
}
impl Count {
    pub fn new(device: &wgpu::Device, workgroup_info: WorkgroupInfo, buffer_capacity: u32) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("count.wgsl"));

        let input = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("{Count, Reorder}::input"),
            size: U32_SIZE.get().checked_mul(buffer_capacity.into()).unwrap(),
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

        let counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("{Count, PartialSum}::counts"),
            size: U32_SIZE.get() * u64::from(workgroup_info.max_workgroups) * u64::from(BASE),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST, // TODO: remove COPY_DST
            mapped_at_creation: false,
        });
        let counts_layout = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let counts_entry = wgpu::BindGroupEntry {
            binding: counts_layout.binding,
            resource: counts.as_entire_binding(),
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Count bind_group_layout"),
            entries: &[input_layout, counts_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Count::bind_group"),
            layout: &bind_group_layout,
            entries: &[input_entry, counts_entry],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Count pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: IMMEDIATES_SIZE,
        });
        let compilation_constants = [("workgroup_size", workgroup_info.workgroup_size.into())];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Count::pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("count"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &compilation_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        Self {
            input,
            counts,
            bind_group,
            pipeline,
            workgroup_info,
        }
    }

    #[must_use]
    fn add_step(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        buffer_len: u32,
        bit_offset: u32,
    ) -> u32 {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Count compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let block_size = buffer_len.div_ceil(self.workgroup_info.workgroup_size);
        let workgroups = buffer_len
            .div_ceil(self.workgroup_info.workgroup_size * block_size)
            .min(buffer_len);
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&CountImmediates {
                buffer_len,
                block_size,
                bit_offset,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, 1, 1);

        workgroups
    }
}

struct PartialSum {
    counts: wgpu::Buffer,
    offsets: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    workgroup_size: u32,
}
impl PartialSum {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        counts: &wgpu::Buffer,
    ) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("partial_sum.wgsl"));

        let counts_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let counts_entry = wgpu::BindGroupEntry {
            binding: counts_layout.binding,
            resource: counts.as_entire_binding(),
        };

        let offsets = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("{PartialSum, Reorder}::offsets"),
            size: counts.size(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let offsets_layout = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let offsets_entry = wgpu::BindGroupEntry {
            binding: offsets_layout.binding,
            resource: offsets.as_entire_binding(),
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("PartialSum bind_group_layout"),
            entries: &[counts_layout, offsets_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("PartialSum::bind_group"),
            layout: &bind_group_layout,
            entries: &[counts_entry, offsets_entry],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("PartialSum pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: IMMEDIATES_SIZE,
        });
        let compilation_constants = [("workgroup_size", workgroup_info.workgroup_size.into())];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("PartialSum::pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("partial_sum"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &compilation_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        Self {
            counts: counts.clone(),
            offsets,
            bind_group,
            pipeline,
            workgroup_size: workgroup_info.workgroup_size,
        }
    }

    fn add_step(&self, encoder: &mut wgpu::CommandEncoder, workgroups: u32) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("PartialSum compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let count_len = workgroups * BASE;
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&PartialSumImmediates {
                count_len,
                block_size: count_len.div_ceil(self.workgroup_size),
            }),
        );

        compute_pass.dispatch_workgroups(1, 1, 1);
    }
}

struct Reorder {
    input: wgpu::Buffer,
    output: wgpu::Buffer,
    offsets: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
    workgroup_info: WorkgroupInfo,
}
impl Reorder {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        input: &wgpu::Buffer,
        offsets: &wgpu::Buffer,
    ) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("reorder.wgsl"));

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

        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Reorder::output"),
            size: input.size(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let output_layout = wgpu::BindGroupLayoutEntry {
            binding: 1,
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

        let offsets_layout = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let offsets_entry = wgpu::BindGroupEntry {
            binding: offsets_layout.binding,
            resource: offsets.as_entire_binding(),
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Reorder bind_group_layout"),
            entries: &[input_layout, output_layout, offsets_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Reorder::bind_group"),
            layout: &bind_group_layout,
            entries: &[input_entry, output_entry, offsets_entry],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Reorder pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: IMMEDIATES_SIZE,
        });
        let compilation_constants = [("workgroup_size", workgroup_info.workgroup_size.into())];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Reorder::pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("reorder"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &compilation_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        Self {
            input: input.clone(),
            output,
            offsets: offsets.clone(),
            bind_group,
            pipeline,
            workgroup_info,
        }
    }

    fn add_step(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32, bit_offset: u32) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Reorder compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let block_size = buffer_len.div_ceil(self.workgroup_info.workgroup_size);
        let workgroups = buffer_len
            .div_ceil(self.workgroup_info.workgroup_size * block_size)
            .min(buffer_len);
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&CountImmediates {
                buffer_len,
                block_size,
                bit_offset,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, 1, 1);
    }
}

struct Sort {
    count: Count,
    partial_sum: PartialSum,
    reorder: Reorder,
}
impl Sort {
    fn new(device: &wgpu::Device, workgroup_info: WorkgroupInfo, buffer_capacity: u32) -> Self {
        let count = Count::new(device, workgroup_info, buffer_capacity);
        let partial_sum = PartialSum::new(device, workgroup_info, &count.counts);
        let reorder = Reorder::new(device, workgroup_info, &count.input, &partial_sum.offsets);

        Self {
            count,
            partial_sum,
            reorder,
        }
    }

    #[inline]
    fn add_copy_output_to_input(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32) {
        encoder.copy_buffer_to_buffer(
            &self.reorder.output,
            0,
            &self.count.input,
            0,
            U32_SIZE.get() * u64::from(buffer_len),
        );
    }

    fn add_step(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32, bit_offset: u32) {
        let workgroups = self.count.add_step(encoder, buffer_len, bit_offset);
        self.partial_sum.add_step(encoder, workgroups);
        self.reorder.add_step(encoder, buffer_len, bit_offset);
    }

    fn add_steps(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32) {
        for bit_offset in (0..u32::BITS).step_by(2) {
            if bit_offset != 0 {
                self.add_copy_output_to_input(encoder, buffer_len);
            }
            self.add_step(encoder, buffer_len, bit_offset);
        }
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct CountImmediates {
    buffer_len: u32,
    block_size: u32,
    bit_offset: u32,
}
#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct PartialSumImmediates {
    count_len: u32,
    block_size: u32,
}
const IMMEDIATES_SIZE: u32 = const_max_u32(
    const_usize_to_u32(size_of::<CountImmediates>()),
    const_usize_to_u32(size_of::<PartialSumImmediates>()),
);

const fn const_usize_to_u32(value: usize) -> u32 {
    if size_of::<u32>() >= size_of::<usize>() {
        return value as u32;
    }
    if value > u32::MAX as usize {
        panic!();
    }
    value as u32
}
const fn const_max_u32(a: u32, b: u32) -> u32 {
    if a > b { a } else { b }
}
