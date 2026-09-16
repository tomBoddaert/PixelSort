use pixel_sort::{
    Config, ConfigBuffer, Vec2U32,
    utils::{U32_SIZE, U64_SIZE},
};
use wgpu::util::DeviceExt;

mod framework;

fn main() {
    framework::run::<PixelSort>();
}

struct PixelSort {
    image_size: Vec2U32,
}

impl framework::Example for PixelSort {
    type Immediates = Immediates;

    fn get_max_buffer_size(image_size: Vec2U32) -> u64 {
        image_size
            .product()
            .checked_mul(U64_SIZE.get() * 2)
            .unwrap()
    }

    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        workgroup_size: u32,
        image: wgpu::Buffer,
        image_size: Vec2U32,
    ) -> (
        Self,
        wgpu::ShaderModule,
        wgpu::BindGroupLayout,
        wgpu::BindGroup,
    ) {
        let ps = pixel_sort::PixelSort::new(device, workgroup_size, image_size.product()).unwrap();

        let image_layout = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(U32_SIZE),
            },
            count: None,
        };
        let image_entry = wgpu::BindGroupEntry {
            binding: image_layout.binding,
            resource: ps.output.as_entire_binding(),
        };

        const CONFIG: Config = {
            let mut buf = ConfigBuffer::new();
            assert!(buf.push_sorted(0, true).is_ok());
            assert!(buf.push_sorted((0.4 * 255.) as u8, false).is_ok());
            assert!(buf.push_sorted((0.6 * 255.) as u8, true).is_ok());
            assert!(buf.push_sorted((0.75 * 255.) as u8, true).is_ok());
            buf.finish()
        };
        let config_upload = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&CONFIG),
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        ps.copy_to_input(&mut encoder, &image, image_size).unwrap();
        ps.copy_to_config(&mut encoder, &config_upload).unwrap();
        ps.add_step(&mut encoder, image_size).unwrap();
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
            entries: &[image_layout],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[image_entry],
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
