use crate::{BASE, U32_SIZE, WorkgroupInfo, const_size_of_u32};

pub struct PartialSum {
    pub counts: wgpu::Buffer,
    pub offsets: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_size: u32,
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
            immediate_size: const { const_size_of_u32::<Immediates>() },
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

    pub fn add_step(&self, encoder: &mut wgpu::CommandEncoder, workgroups: u32) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("PartialSum compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let count_len = workgroups * BASE;
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                count_len,
                block_size: count_len.div_ceil(self.workgroup_size),
            }),
        );

        compute_pass.dispatch_workgroups(1, 1, 1);
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub count_len: u32,
    pub block_size: u32,
}
