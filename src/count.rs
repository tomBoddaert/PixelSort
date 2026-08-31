use crate::{BASE, U32_SIZE, WorkgroupInfo, const_size_of_u32};

pub struct Count {
    pub input: wgpu::Buffer,
    pub counts: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_info: WorkgroupInfo,
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
            immediate_size: const { const_size_of_u32::<Immediates>() },
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
    pub fn add_step(
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
            bytemuck::bytes_of(&Immediates {
                buffer_len,
                block_size,
                bit_offset,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, 1, 1);

        workgroups
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub buffer_len: u32,
    pub block_size: u32,
    pub bit_offset: u32,
}
