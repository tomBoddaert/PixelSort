use pixel_sort::{U64_SIZE, Vec2U32, WorkgroupInfo, section::Section, sort::Sort};

mod framework;

fn main() {
    framework::run::<PixelSort>();
}

struct PixelSort {
    image_size: Vec2U32,
}

impl framework::Example for PixelSort {
    type Immediates = Immediates;

    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        workgroup_info: WorkgroupInfo,
        image: wgpu::Buffer,
        image_size: Vec2U32,
    ) -> (
        Self,
        wgpu::ShaderModule,
        wgpu::BindGroupLayout,
        wgpu::BindGroup,
    ) {
        let section = Section::new(device, workgroup_info, image_size);
        let sort = Sort::new(device, workgroup_info, image_size, section.tagged_image());

        let tagged_image_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U64_SIZE),
            },
            count: None,
        };
        let tagged_image_entry = wgpu::BindGroupEntry {
            binding: tagged_image_layout.binding,
            resource: sort.tagged_image().as_entire_binding(),
        };

        const THRESHOLD: f32 = 0.4;

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        section.add_step(&mut encoder, image_size, THRESHOLD, &image);
        sort.add_step(&mut encoder, image_size);
        let idx = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(idx),
                timeout: None,
            })
            .unwrap();

        let module = device.create_shader_module(wgpu::include_wgsl!("sort.wgsl"));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[tagged_image_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[tagged_image_entry],
        });

        (Self { image_size }, module, bind_group_layout, bind_group)
    }

    fn immediates(&self, window_size: Vec2U32) -> Self::Immediates {
        Immediates {
            size: window_size,
            image_size: self.image_size,
        }
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct Immediates {
    size: Vec2U32,
    image_size: Vec2U32,
}
