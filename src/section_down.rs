use crate::{
    TEST_COPY_SRC, U32_SIZE, U64_SIZE, Vec2U32, WorkgroupInfo, const_size_of_u32, section_up,
};

pub struct SectionDown {
    pub max_image_size: Vec2U32,
    pub image_buffer: wgpu::Buffer,
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
        max_image_size: Vec2U32,
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
            label: Some("{SectionDown, Count, Reorder}::tagged_image"),
            size: max_image_size
                .product()
                .checked_mul(U64_SIZE.get())
                .unwrap(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | TEST_COPY_SRC,
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
            max_image_size,
            image_buffer: image.clone(),
            tagged_image,
            left_workgroup: left_workgroup.clone(),
            workgroup_right: workgroup_right.clone(),
            bind_group,
            pipeline,
            workgroup_info,
        }
    }

    pub fn add_step(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image_size: Vec2U32,
        threshold: f32,
    ) {
        let pixels = image_size.product();
        // Intentionally allow wider images within pixel limit
        assert!(pixels <= self.max_image_size.product());
        assert!(image_size.y <= self.max_image_size.y);

        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("SectionDown compute_pass"),
            timestamp_writes: None,
        });

        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_bind_group(0, &self.bind_group, &[]);
        let block_size = image_size.x.div_ceil(
            self.workgroup_info
                .max_workgroups
                .checked_mul(self.workgroup_info.workgroup_size)
                .unwrap(),
        );
        let workgroups = image_size.x.div_ceil(
            self.workgroup_info
                .workgroup_size
                .checked_mul(block_size)
                .unwrap(),
        );
        compute_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                width: image_size.x,
                block_size,
                threshold,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, image_size.y, 1);
    }
}

pub type Immediates = section_up::Immediates;

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
        const WHITE: u32 = 0x00ffffff;
        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
                WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE,
                WHITE, WHITE, WHITE, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK,
                BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, //
                BLACK, WHITE, BLACK, BLACK, WHITE, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK,
                BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE,
                BLACK, WHITE, BLACK, WHITE, WHITE, WHITE,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let image_size = Vec2U32 { x: 30, y: 2 };
        let left_workgroup = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
                0, 1, 1, 1, //
                0, 0, 2, 3,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let workgroup_right = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
                0,
                15,
                u32::MAX,
                u32::MAX, //
                5,
                u32::MAX,
                18,
                27,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let section_down = SectionDown::new(
            device,
            workgroup_info,
            image_size,
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
        section_down.add_step(&mut encoder, image_size, 0.5);
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
        let result = bytemuck::cast_slice::<u8, u32>(&downloaded);

        assert_eq!(
            result,
            &[
                0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE,
                0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE,
                0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE, 0x000ff, WHITE,
                0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK,
                0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK,
                0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK, 0xf0000, BLACK, 0xf0000,
                BLACK, //
                0x00000, BLACK, 0x100ff, WHITE, 0x20000, BLACK, 0x20000, BLACK, 0x400ff, WHITE,
                0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK,
                0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK,
                0x50000, BLACK, 0x50000, BLACK, 0x50000, BLACK, 0x1200ff, WHITE, 0x1200ff, WHITE,
                0x1200ff, WHITE, 0x1200ff, WHITE, 0x1200ff, WHITE, 0x1200ff, WHITE, 0x180000,
                BLACK, 0x1900ff, WHITE, 0x1a0000, BLACK, 0x1b00ff, WHITE, 0x1b00ff, WHITE,
                0x1b00ff, WHITE,
            ],
        );
    }
}
