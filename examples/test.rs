use pixel_sort::{
    IMMEDIATES_SIZE, Vec2U32, WorkgroupInfo, section_down, section_global, section_up,
};
use wgpu::util::DeviceExt;

fn main() {
    env_logger::init();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let downlevel_capabilities = adapter.get_downlevel_capabilities();
    if !downlevel_capabilities
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    {
        panic!("Adapter does not support compute shaders");
    }

    let mut required_limits = wgpu::Limits::defaults();
    required_limits.max_immediate_size = IMMEDIATES_SIZE;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: wgpu::Features::IMMEDIATES | wgpu::Features::SUBGROUP,
        required_limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
    }))
    .expect("Failed to create device");

    let workgroup_info = WorkgroupInfo {
        workgroup_size: 2,
        max_workgroups: 4,
    };

    const BLACK: u32 = 0x00000000;
    const WHITE: u32 = 0xffffff00;
    let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice::<u32, u8>(&[
            BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK,
            BLACK, BLACK, BLACK, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE,
            WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, //
            BLACK, WHITE, BLACK, BLACK, WHITE, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK,
            BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE,
            BLACK, WHITE, BLACK, WHITE, WHITE, WHITE,
        ]),
        usage: wgpu::BufferUsages::STORAGE,
    });

    let image_size = Vec2U32 { x: 30, y: 2 };
    let section_up = section_up::SectionUp::new(&device, workgroup_info, image_size, &image);
    let section_global = section_global::SectionGlobal::new(
        &device,
        workgroup_info,
        &section_up.left_workgroup,
        image_size.y,
    );
    let section_down = section_down::SectionDown::new(
        &device,
        workgroup_info,
        image_size,
        &image,
        &section_up.left_workgroup,
        &section_up.workgroup_right,
    );

    let download = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: section_down.tagged_image.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let workgroups = section_up.add_step(&mut encoder);
    section_global.add_step(&mut encoder, workgroups);
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
    println!("{:?}", result.iter().map(|v| v.x >> 16).collect::<Vec<_>>());
    // println!("{result:?}");
}
