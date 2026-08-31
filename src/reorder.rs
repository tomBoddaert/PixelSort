use crate::{U32_SIZE, WorkgroupInfo, const_size_of_u32, count::Immediates};

pub struct Reorder {
    pub input: wgpu::Buffer,
    pub output: wgpu::Buffer,
    pub offsets: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_info: WorkgroupInfo,
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
            immediate_size: const { const_size_of_u32::<Immediates>() },
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

    pub fn add_step(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32, bit_offset: u32) {
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
            bytemuck::bytes_of(&Immediates {
                buffer_len,
                block_size,
                bit_offset,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, 1, 1);
    }
}
