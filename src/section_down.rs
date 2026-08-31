use crate::{
    U32_SIZE, U64_SIZE, Vec2U32, WorkgroupInfo, const_size_of_u32, section_up::Immediates,
};

pub struct SectionDown {
    pub image_size: Vec2U32,
    pub image: wgpu::Buffer,
    pub tagged_image: wgpu::Buffer,
    pub left_workgroup: wgpu::Buffer,
    pub workgroup_right: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_info: WorkgroupInfo,
}

impl SectionDown {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        image_size: Vec2U32,
        image: &wgpu::Buffer,
        left_workgroup: &wgpu::Buffer,
        workgroup_right: &wgpu::Buffer,
    ) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("section_down.wgsl"));

        let image_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let image_entry = wgpu::BindGroupEntry {
            binding: image_layout.binding,
            resource: image.as_entire_binding(),
        };

        let tagged_image = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SectionDown::tagged_image"),
            size: image.size().checked_mul(2).unwrap(), // TODO: replace this
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
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

        let left_workgroup_layout = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let left_workgroup_entry = wgpu::BindGroupEntry {
            binding: left_workgroup_layout.binding,
            resource: left_workgroup.as_entire_binding(),
        };

        let workgroup_right_layout = wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let workgroup_right_entry = wgpu::BindGroupEntry {
            binding: workgroup_right_layout.binding,
            resource: workgroup_right.as_entire_binding(),
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("SectionDown bind_group_layout"),
            entries: &[
                image_layout,
                tagged_image_layout,
                left_workgroup_layout,
                workgroup_right_layout,
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SectionDown::bind_group"),
            layout: &bind_group_layout,
            entries: &[
                image_entry,
                tagged_image_entry,
                left_workgroup_entry,
                workgroup_right_entry,
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SectionDown pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: const { const_size_of_u32::<Immediates>() },
        });
        let compilation_constants = [("workgroup_size", workgroup_info.workgroup_size.into())];
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("SectionDown::pipeline"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("section_down"),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &compilation_constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
            cache: None,
        });

        Self {
            image_size,
            image: image.clone(),
            tagged_image,
            left_workgroup: left_workgroup.clone(),
            workgroup_right: workgroup_right.clone(),
            bind_group,
            pipeline,
            workgroup_info,
        }
    }

    pub fn add_step(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("SectionDown compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let block_size = self.image_size.x.div_ceil(
            self.workgroup_info
                .max_workgroups
                .checked_mul(self.workgroup_info.workgroup_size)
                .unwrap(),
        );
        let workgroups = self.image_size.x.div_ceil(
            self.workgroup_info
                .workgroup_size
                .checked_mul(block_size)
                .unwrap(),
        );
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                width: self.image_size.x,
                block_size,
                threshold: 0.4,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, self.image_size.y, 1);
    }
}

#[cfg(test)]
mod test {
    use wgpu::util::DeviceExt;

    use crate::{Vec2U32, WorkgroupInfo, section_down::SectionDown};

    #[test]
    fn basic() {
        let crate::test::State { device, queue } = crate::test::get_state();
        let workgroup_info = WorkgroupInfo {
            workgroup_size: 2,
            max_workgroups: 4,
        };

        const BLACK: u32 = 0x00000000;
        const WHITE: u32 = 0xffffff00;
        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
                BLACK, WHITE, BLACK, BLACK, WHITE, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK,
                BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE,
                BLACK, WHITE, BLACK, WHITE, WHITE, WHITE,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let left_workgroup = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[0, 0, 2, 3]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let workgroup_right = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[5, u32::MAX, 18, 27]),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let section_down = SectionDown::new(
            device,
            workgroup_info,
            Vec2U32 { x: 30, y: 1 },
            &image,
            &left_workgroup,
            &workgroup_right,
        );

        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: section_down.tagged_image.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        section_down.add_step(&mut encoder);
        encoder.copy_buffer_to_buffer(
            &section_down.tagged_image,
            0,
            &download,
            0,
            section_down.tagged_image.size(),
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
        let result = bytemuck::cast_slice::<u8, Vec2U32>(&downloaded);

        assert_eq!(
            &*result.iter().map(|v| v.x >> 16).collect::<Box<[_]>>(),
            &[
                0, 1, 2, 2, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 18, 18, 18, 18, 18, 18, 24,
                25, 26, 27, 27, 27
            ],
        );
    }
}
