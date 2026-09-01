use crate::{U32_SIZE, WorkgroupInfo, const_size_of_u32};

pub struct SectionGlobal {
    pub max_image_height: u32,
    pub left_workgroup: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_info: WorkgroupInfo,
}

impl SectionGlobal {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        left_workgroup: &wgpu::Buffer,
        max_image_height: u32,
    ) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("section_global.wgsl"));

        let left_workgroup_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let left_workgroup_entry = wgpu::BindGroupEntry {
            binding: left_workgroup_layout.binding,
            resource: left_workgroup.as_entire_binding(),
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("SectionGlobal bind_group_layout"),
            entries: &[left_workgroup_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SectionGlobal::bind_group"),
            layout: &bind_group_layout,
            entries: &[left_workgroup_entry],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SectionGlobal pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: const { const_size_of_u32::<Immediates>() },
        });
        let compilation_constants = [("workgroup_size", workgroup_info.workgroup_size.into())];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("SectionGlobal::pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("section_global"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &compilation_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        Self {
            max_image_height,
            left_workgroup: left_workgroup.clone(),
            bind_group,
            pipeline,
            workgroup_info,
        }
    }

    pub fn add_step(&self, encoder: &mut wgpu::CommandEncoder, image_height: u32, workgroups: u32) {
        assert!(image_height <= self.max_image_height);
        assert!(workgroups <= self.workgroup_info.max_workgroups);

        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("SectionGlobal compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                workgroups,
                block_size: workgroups.div_ceil(self.workgroup_info.workgroup_size),
            }),
        );

        compute_pass.dispatch_workgroups(1, image_height, 1);
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub workgroups: u32,
    pub block_size: u32,
}

#[cfg(test)]
mod test {
    use wgpu::util::DeviceExt;

    use crate::{TEST_COPY_SRC, WorkgroupInfo, section_global::SectionGlobal};

    #[test]
    fn basic() {
        let crate::test::State { device, queue } = crate::test::get_state();
        let workgroup_info = WorkgroupInfo {
            workgroup_size: 4,
            max_workgroups: 16,
        };

        let left_workgroup = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
                0, 0, 2, 2, 3, 4, 5, 6, 7, 9, 10, 11, 11, 12, 14, 15, //
                0, 1, 1, 2, 3, 5, 5, 6, 8, 9, 10, 11, 11, 12, 13, 14,
            ]),
            usage: wgpu::BufferUsages::STORAGE | TEST_COPY_SRC,
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: left_workgroup.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let section_global = SectionGlobal::new(device, workgroup_info, &left_workgroup, 2);

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        section_global.add_step(&mut encoder, 2, 16);
        encoder.copy_buffer_to_buffer(&left_workgroup, 0, &download, 0, left_workgroup.size());
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
                0, 0, 2, 2, 2, 2, 2, 2, 2, 9, 10, 11, 11, 11, 14, 15, //
                0, 1, 1, 1, 1, 5, 5, 5, 8, 9, 10, 11, 11, 11, 11, 11,
            ],
        );
    }
}
