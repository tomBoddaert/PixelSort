use crate::{BASE, TEST_COPY_SRC, U32_SIZE, Vec2U32, WorkgroupInfo, const_size_of_u32};

pub struct PartialSum {
    pub max_image_height: u32,
    pub counts: wgpu::Buffer,
    pub offsets: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_info: WorkgroupInfo,
}

impl PartialSum {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        counts: &wgpu::Buffer,
        max_image_height: u32,
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
            size: Vec2U32 {
                x: workgroup_info.max_workgroups,
                y: max_image_height,
            }
            .product()
            .checked_mul(U32_SIZE.get() * u64::from(BASE))
            .unwrap(),
            usage: wgpu::BufferUsages::STORAGE | TEST_COPY_SRC,
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
            max_image_height,
            counts: counts.clone(),
            offsets,
            bind_group,
            pipeline,
            workgroup_info,
        }
    }

    pub fn add_step(&self, encoder: &mut wgpu::CommandEncoder, image_height: u32, workgroups: u32) {
        assert!(workgroups <= self.workgroup_info.max_workgroups);
        assert!(image_height <= self.max_image_height);

        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("PartialSum compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let count_len = workgroups.checked_mul(BASE).unwrap();
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                count_len,
                block_size: count_len.div_ceil(self.workgroup_info.workgroup_size),
            }),
        );

        compute_pass.dispatch_workgroups(1, image_height, 1);
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub count_len: u32,
    pub block_size: u32,
}

#[cfg(test)]
mod test {
    use wgpu::util::DeviceExt;

    use crate::{WorkgroupInfo, partial_sum::PartialSum};

    #[test]
    fn basic() {
        let crate::test::State { device, queue } = crate::test::get_state();
        let workgroup_info = WorkgroupInfo {
            workgroup_size: 2,
            max_workgroups: 4,
        };

        let counts = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
                0, 1, 8, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 8, 7, 0, 0, //
                6, 8, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 2, 0, 6, 4,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let image_height = 2;

        let partial_sum = PartialSum::new(device, workgroup_info, &counts, image_height);

        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: partial_sum.offsets.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        partial_sum.add_step(&mut encoder, image_height, 4);
        encoder.copy_buffer_to_buffer(
            &partial_sum.offsets,
            0,
            &download,
            0,
            partial_sum.offsets.size(),
        );
        encoder.map_buffer_on_submit(&download, wgpu::MapMode::Read, .., |_| {});
        let ix = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(ix),
                timeout: None,
            })
            .unwrap();

        let downloaded = download.get_mapped_range(..).unwrap();
        let result = bytemuck::cast_slice::<u8, u32>(&downloaded);

        assert_eq!(
            result,
            &[
                0, 0, 1, 9, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
                15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
                15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 23, 30,
                30, //
                0, 6, 14, 16, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
                18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18,
                18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 20, 20,
                26,
            ]
        );
    }
}
