use crate::{BASE, TEST_COPY_SRC, U32_SIZE, U64_SIZE, Vec2U32, WorkgroupInfo, const_size_of_u32};

pub struct Count {
    pub max_image_size: Vec2U32,
    pub tagged_image: wgpu::Buffer,
    pub counts: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
    pub pipeline: wgpu::ComputePipeline,
    pub workgroup_info: WorkgroupInfo,
}

impl Count {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        max_image_size: Vec2U32,
        tagged_image: &wgpu::Buffer,
    ) -> Self {
        let module = device.create_shader_module(wgpu::include_wgsl!("count.wgsl"));

        let tagged_image_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U64_SIZE),
            },
            count: None,
        };
        let tagged_image_entry = wgpu::BindGroupEntry {
            binding: tagged_image_layout.binding,
            resource: tagged_image.as_entire_binding(),
        };

        let counts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("{Count, PartialSum}::counts"),
            size: Vec2U32 {
                x: workgroup_info.max_workgroups,
                y: max_image_size.y,
            }
            .product()
            .checked_mul(U32_SIZE.get() * u64::from(BASE))
            .unwrap(),
            usage: wgpu::BufferUsages::STORAGE | TEST_COPY_SRC,
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
            entries: &[tagged_image_layout, counts_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Count::bind_group"),
            layout: &bind_group_layout,
            entries: &[tagged_image_entry, counts_entry],
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
            max_image_size,
            tagged_image: tagged_image.clone(),
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
        image_size: Vec2U32,
        bit_offset: u32,
    ) -> u32 {
        let pixels = image_size.product();
        // Intentionally allow wider images within pixel limit
        assert!(pixels <= self.max_image_size.product());
        assert!(image_size.y <= self.max_image_size.y);

        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Count compute_pass"),
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
                bit_offset,
            }),
        );

        compute_pass.dispatch_workgroups(workgroups, image_size.y, 1);

        workgroups
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub width: u32,
    pub block_size: u32,
    pub bit_offset: u32,
}

#[cfg(test)]
mod test {
    use wgpu::util::DeviceExt;

    use crate::{Vec2U32, WorkgroupInfo, count::Count};

    #[test]
    fn basic() {
        let crate::test::State { device, queue } = crate::test::get_state();
        let workgroup_info = WorkgroupInfo {
            workgroup_size: 2,
            max_workgroups: 4,
        };

        const BLACK: u32 = 0x00000000;
        const WHITE: u32 = 0x00ffffff;
        let tagged_image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<u32, u8>(&[
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
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let image_size = Vec2U32 { x: 30, y: 2 };

        let count = Count::new(device, workgroup_info, image_size, &tagged_image);

        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: count.counts.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        assert_eq!(count.add_step(&mut encoder, image_size, 0), 4);
        encoder.copy_buffer_to_buffer(&count.counts, 0, &download, 0, count.counts.size());
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
                0, 1, 8, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 8, 7, 0, 0, //
                6, 8, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 2, 0, 6, 4,
            ]
        );
    }
}
