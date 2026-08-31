use std::num::NonZero;

pub mod count;
pub mod partial_sum;
pub mod render;
pub mod reorder;
pub mod section_down;
pub mod section_global;
pub mod section_up;

pub const U32_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u32>())).unwrap();
pub const U64_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u64>())).unwrap();
pub const BIT_LEN: u32 = 4;
pub const BASE: u32 = 2_u32.pow(BIT_LEN);

// fn main() {
//     env_logger::init();

//     let event_loop = winit::event_loop::EventLoop::new().unwrap();
//     event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

//     let mut app = App::default();
//     event_loop.run_app(&mut app).unwrap();
// }

// #[derive(Default)]
// struct App {
//     state: Option<State>,
// }

// struct State {
//     window: Arc<winit::window::Window>,
//     instance: wgpu::Instance,
//     adapter: wgpu::Adapter,
//     device: wgpu::Device,
//     queue: wgpu::Queue,
//     size: winit::dpi::PhysicalSize<u32>,
//     surface: wgpu::Surface<'static>,
//     surface_format: wgpu::TextureFormat,
//     workgroup_info: WorkgroupInfo,
//     source_image_size: Vec2U32,
//     render_bind_group: wgpu::BindGroup,
//     pipeline_layout: wgpu::PipelineLayout,
//     pipeline: wgpu::RenderPipeline,
// }

// impl winit::application::ApplicationHandler for App {
//     fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
//         let window = Arc::new(
//             event_loop
//                 .create_window(Window::default_attributes())
//                 .unwrap(),
//         );

//         self.state = Some(State::new(event_loop.owned_display_handle(), window));
//     }

//     fn window_event(
//         &mut self,
//         event_loop: &winit::event_loop::ActiveEventLoop,
//         window_id: winit::window::WindowId,
//         event: winit::event::WindowEvent,
//     ) {
//         let state = self.state.as_mut().unwrap();
//         match event {
//             winit::event::WindowEvent::CloseRequested => {
//                 event_loop.exit();
//             }
//             winit::event::WindowEvent::RedrawRequested => {
//                 state.draw();
//                 state.window.request_redraw();
//             }
//             winit::event::WindowEvent::Resized(size) => {
//                 // Reconfigures the size of the surface. We do not re-render
//                 // here as this event is always followed up by redraw request.
//                 state.resize(size);
//             }

//             winit::event::WindowEvent::KeyboardInput {
//                 device_id,
//                 event,
//                 is_synthetic,
//             } => match event.logical_key {
//                 winit::keyboard::Key::Character(c) if c == "q" => {
//                     event_loop.exit();
//                 }
//                 _ => {}
//             },
//             _ => (),
//         }
//     }
// }

// impl State {
//     fn new(
//         display: winit::event_loop::OwnedDisplayHandle,
//         window: Arc<winit::window::Window>,
//     ) -> Self {
//         let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
//             Box::new(display),
//         ));

//         let adapter =
//             pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
//                 .unwrap();
//         let downlevel_capabilities = adapter.get_downlevel_capabilities();
//         if !downlevel_capabilities
//             .flags
//             .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
//         {
//             panic!("Adapter does not support compute shaders");
//         }

//         let mut required_limits = wgpu::Limits::defaults();
//         required_limits.max_immediate_size = IMMEDIATES_SIZE;

//         let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
//             label: None,
//             required_features: wgpu::Features::IMMEDIATES | wgpu::Features::SUBGROUP,
//             required_limits,
//             experimental_features: wgpu::ExperimentalFeatures::disabled(),
//             memory_hints: wgpu::MemoryHints::MemoryUsage,
//             trace: wgpu::Trace::Off,
//         }))
//         .expect("Failed to create device");

//         let size = window.inner_size();

//         let surface = instance.create_surface(Arc::clone(&window)).unwrap();
//         let surface_capabilities = surface.get_capabilities(&adapter);
//         let surface_format = surface_capabilities.formats[0];

//         let workgroup_info = WorkgroupInfo {
//             workgroup_size: adapter.get_info().subgroup_max_size,
//             max_workgroups: 32,
//         };

//         let img = image::ImageReader::open("source.jpg")
//             .unwrap()
//             .decode()
//             .unwrap();
//         let img = img.into_rgba8();
//         let source_image_size = Vec2U32 {
//             x: img.width(),
//             y: img.height(),
//         };

//         // let section_workgroups = SectionWorkgroup::new(&device, workgroup_info, img);

//         // let sections_layout = wgpu::BindGroupLayoutEntry {
//         //     binding: 1,
//         //     visibility: wgpu::ShaderStages::FRAGMENT,
//         //     ty: wgpu::BindingType::Buffer {
//         //         ty: wgpu::BufferBindingType::Storage { read_only: true },
//         //         has_dynamic_offset: false,
//         //         min_binding_size: Some(U32_SIZE),
//         //     },
//         //     count: None,
//         // };
//         // let sections_entry = wgpu::BindGroupEntry {
//         //     binding: 1,
//         //     resource: section_workgroups.workgroup_regions.as_entire_binding(),
//         // };

//         // --
//         let render_module = device.create_shader_module(wgpu::include_wgsl!("render.wgsl"));

//         let source_image_layout = wgpu::BindGroupLayoutEntry {
//             binding: 0,
//             visibility: wgpu::ShaderStages::FRAGMENT,
//             ty: wgpu::BindingType::Buffer {
//                 ty: wgpu::BufferBindingType::Storage { read_only: true },
//                 has_dynamic_offset: false,
//                 min_binding_size: Some(U32_SIZE),
//             },
//             count: None,
//         };
//         let source_image_entry = wgpu::BindGroupEntry {
//             binding: source_image_layout.binding,
//             resource: section_workgroups.image.as_entire_binding(),
//         };

//         let sections_layout = wgpu::BindGroupLayoutEntry {
//             binding: 1,
//             visibility: wgpu::ShaderStages::FRAGMENT,
//             ty: wgpu::BindingType::Buffer {
//                 ty: wgpu::BufferBindingType::Storage { read_only: true },
//                 has_dynamic_offset: false,
//                 min_binding_size: Some(U32_SIZE),
//             },
//             count: None,
//         };
//         let sections_entry = wgpu::BindGroupEntry {
//             binding: sections_layout.binding,
//             resource: section_workgroups.workgroup_regions.as_entire_binding(),
//         };

//         let render_bind_group_layout =
//             device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
//                 label: None,
//                 entries: &[source_image_layout, sections_layout],
//             });
//         let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
//             label: None,
//             layout: &render_bind_group_layout,
//             entries: &[source_image_entry, sections_entry],
//         });

//         let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
//             label: None,
//             bind_group_layouts: &[Some(&render_bind_group_layout)],
//             immediate_size: const { const_size_of_u32::<render::RenderImmediates>() },
//         });
//         let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
//             label: None,
//             layout: Some(&pipeline_layout),
//             vertex: wgpu::VertexState {
//                 module: &render_module,
//                 entry_point: Some("vertex"),
//                 compilation_options: wgpu::PipelineCompilationOptions::default(),
//                 buffers: &[],
//             },
//             primitive: wgpu::PrimitiveState::default(),
//             depth_stencil: None,
//             multisample: wgpu::MultisampleState::default(),
//             fragment: Some(wgpu::FragmentState {
//                 module: &render_module,
//                 entry_point: Some("fragment"),
//                 compilation_options: wgpu::PipelineCompilationOptions::default(),
//                 targets: &[Some(surface_format.add_srgb_suffix().into())],
//             }),
//             multiview_mask: None,
//             cache: None,
//         });

//         let mut encoder =
//             device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
//         let _ = section_workgroups.add_step(&mut encoder);
//         let idx = queue.submit([encoder.finish()]);

//         device
//             .poll(wgpu::PollType::Wait {
//                 submission_index: Some(idx),
//                 timeout: None,
//             })
//             .unwrap();

//         // --

//         // let test_values: [u32; _] = [
//         //     38, 10, 34, 67, 27, 16, 63, 36, 29, 68, 19, 67, 16, 94, 39, 42, 14, 4, 13, 15, 11, 60,
//         //     11, 72, 0, 82, 49, 80, 26, 93, 11, 77, 70, 92, 49, 26, 72, 55, 25, 8, 30, 75, 49, 53,
//         //     67, 15, 38, 21, 11, 37, 53, 65, 41, 34, 41, 73, 24, 15, 73, 38, 97, 70, 39, 93, 5, 91,
//         //     88, 72, 25, 98, 79, 11, 53, 54, 38, 26, 58, 10, 35, 37, 55, 82, 33, 58, 57, 14, 49, 65,
//         //     90, 98, 48, 66, 56, 76, 77, 67, 5, 53, 75, 3,
//         // ];
//         // let test_values_len = test_values.len().try_into().unwrap();
//         // let sort = Sort::new(&device, workgroup_info, test_values_len);

//         // let upload = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
//         //     label: Some("upload"),
//         //     contents: bytemuck::cast_slice(&test_values),
//         //     usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
//         // });
//         // let download = device.create_buffer(&wgpu::BufferDescriptor {
//         //     label: Some("download"),
//         //     size: upload.size(),
//         //     usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
//         //     mapped_at_creation: false,
//         // });

//         Self {
//             window,
//             instance,
//             adapter,
//             device,
//             queue,
//             size,
//             surface,
//             surface_format,
//             workgroup_info,
//             source_image_size,
//             render_module,
//             render_bind_group,
//             pipeline_layout,
//             pipeline,
//         }
//     }

//     fn configure_surface(&self) {
//         let surface_config = wgpu::SurfaceConfiguration {
//             usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
//             format: self.surface_format,
//             color_space: wgpu::SurfaceColorSpace::Auto,
//             width: self.size.width,
//             height: self.size.height,
//             present_mode: wgpu::PresentMode::AutoVsync,
//             desired_maximum_frame_latency: 2,
//             alpha_mode: wgpu::CompositeAlphaMode::Auto,
//             view_formats: vec![self.surface_format.add_srgb_suffix()],
//         };
//         self.surface.configure(&self.device, &surface_config);
//     }

//     fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
//         self.size = new_size;
//         self.configure_surface();
//     }

//     fn get_surface_texture(&mut self) -> Option<wgpu::SurfaceTexture> {
//         // Create texture view.
//         // NOTE: We must handle Timeout because the surface may be unavailable
//         // (e.g., when the window is occluded on macOS).
//         Some(match self.surface.get_current_texture() {
//             wgpu::CurrentSurfaceTexture::Success(texture) => texture,
//             wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
//                 return None;
//             }
//             wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
//                 drop(texture);
//                 self.configure_surface();
//                 return None;
//             }
//             wgpu::CurrentSurfaceTexture::Outdated => {
//                 self.configure_surface();
//                 return None;
//             }
//             wgpu::CurrentSurfaceTexture::Validation => {
//                 unreachable!("No error scope registered, so validation errors will panic")
//             }
//             wgpu::CurrentSurfaceTexture::Lost => {
//                 self.surface = self.instance.create_surface(self.window.clone()).unwrap();
//                 self.configure_surface();
//                 return None;
//             }
//         })
//     }

//     fn get_texture_view(&self, surface_texture: &wgpu::SurfaceTexture) -> wgpu::TextureView {
//         surface_texture
//             .texture
//             .create_view(&wgpu::TextureViewDescriptor {
//                 // Without add_srgb_suffix() the image we will be working with
//                 // might not be "gamma correct".
//                 format: Some(self.surface_format.add_srgb_suffix()),
//                 ..Default::default()
//             })
//     }

//     fn draw(&mut self) {
//         let Some(surface_texture) = self.get_surface_texture() else {
//             return;
//         };
//         let texture_view = self.get_texture_view(&surface_texture);

//         let mut encoder = self
//             .device
//             .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

//         let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
//             label: None,
//             color_attachments: &[Some(wgpu::RenderPassColorAttachment {
//                 view: &texture_view,
//                 depth_slice: None,
//                 resolve_target: None,
//                 ops: wgpu::Operations {
//                     load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
//                     store: wgpu::StoreOp::Store,
//                 },
//             })],
//             depth_stencil_attachment: None,
//             timestamp_writes: None,
//             occlusion_query_set: None,
//             multiview_mask: None,
//         });
//         render_pass.set_pipeline(&self.pipeline);
//         render_pass.set_bind_group(0, &self.render_bind_group, &[]);
//         render_pass.set_immediates(
//             0,
//             bytemuck::bytes_of(&RenderImmediates {
//                 size: Vec2U32 {
//                     x: self.size.width,
//                     y: self.size.height,
//                 },
//                 image_size: self.source_image_size,
//             }),
//         );
//         render_pass.draw(0..6, 0..1);
//         drop(render_pass);

//         self.queue.submit([encoder.finish()]);
//         self.window.pre_present_notify();
//         self.queue.present(surface_texture);
//     }
// }

// fn run() {
//     let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
//         label: Some("encoder"),
//     });

//     encoder.copy_buffer_to_buffer(&upload, 0, &sort.count.input, 0, upload.size());

//     sort.add_steps(&mut encoder, test_values_len);

//     encoder.copy_buffer_to_buffer(&sort.reorder.output, 0, &download, 0, download.size());
//     encoder.map_buffer_on_submit(&download, wgpu::MapMode::Read, .., |_| {});

//     let ix = queue.submit([encoder.finish()]);
//     device
//         .poll(wgpu::PollType::Wait {
//             submission_index: Some(ix),
//             timeout: None,
//         })
//         .unwrap();

//     let download_mapped = download.get_mapped_range(..).unwrap();
//     let downloaded = bytemuck::cast_slice::<u8, u32>(&download_mapped);
//     println!("Input:\t{test_values:>2?}");
//     println!("Output:\t{downloaded:>2?}");

//     let mut correct = test_values;
//     correct.sort_unstable();
//     assert_eq!(downloaded, &correct);
// }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkgroupInfo {
    pub workgroup_size: u32,
    pub max_workgroups: u32,
}

// struct Sort {
//     count: Count,
//     partial_sum: PartialSum,
//     reorder: Reorder,
// }
// impl Sort {
//     fn new(device: &wgpu::Device, workgroup_info: WorkgroupInfo, buffer_capacity: u32) -> Self {
//         let count = Count::new(device, workgroup_info, buffer_capacity);
//         let partial_sum = PartialSum::new(device, workgroup_info, &count.counts);
//         let reorder = Reorder::new(device, workgroup_info, &count.input, &partial_sum.offsets);

//         Self {
//             count,
//             partial_sum,
//             reorder,
//         }
//     }

//     #[inline]
//     fn add_copy_output_to_input(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32) {
//         encoder.copy_buffer_to_buffer(
//             &self.reorder.output,
//             0,
//             &self.count.input,
//             0,
//             U32_SIZE.get() * u64::from(buffer_len),
//         );
//     }

//     fn add_step(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32, bit_offset: u32) {
//         let workgroups = self.count.add_step(encoder, buffer_len, bit_offset);
//         self.partial_sum.add_step(encoder, workgroups);
//         self.reorder.add_step(encoder, buffer_len, bit_offset);
//     }

//     fn add_steps(&self, encoder: &mut wgpu::CommandEncoder, buffer_len: u32) {
//         for bit_offset in (0..u32::BITS).step_by(2) {
//             if bit_offset != 0 {
//                 self.add_copy_output_to_input(encoder, buffer_len);
//             }
//             self.add_step(encoder, buffer_len, bit_offset);
//         }
//     }
// }

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Vec2U32 {
    pub x: u32,
    pub y: u32,
}

pub const IMMEDIATES_SIZE: u32 = const_max_u32_slice(&[
    const_size_of_u32::<section_up::Immediates>(),
    const_size_of_u32::<section_global::Immediates>(),
    const_size_of_u32::<count::Immediates>(),
    const_size_of_u32::<partial_sum::Immediates>(),
    const_size_of_u32::<render::Immediates>(),
]);

const fn const_usize_to_u32(value: usize) -> u32 {
    if size_of::<u32>() >= size_of::<usize>() {
        return value as u32;
    }
    if value > u32::MAX as usize {
        panic!();
    }
    value as u32
}
const fn const_usize_to_u64(value: usize) -> u64 {
    if size_of::<u64>() >= size_of::<usize>() {
        return value as u64;
    }
    if value > u64::MAX as usize {
        panic!();
    }
    value as u64
}
const fn const_size_of_u32<T>() -> u32 {
    const_usize_to_u32(size_of::<T>())
}
const fn const_max_u32_slice(s: &[u32]) -> u32 {
    let mut max = 0;
    let mut i = 0;

    while i < s.len() {
        if s[i] > max {
            max = s[i];
        }

        i += 1;
    }

    max
}

#[cfg(test)]
mod test {
    use std::sync::OnceLock;

    use crate::IMMEDIATES_SIZE;

    pub struct State {
        pub device: wgpu::Device,
        pub queue: wgpu::Queue,
    }
    static STATE: OnceLock<State> = OnceLock::new();
    pub fn get_state() -> &'static State {
        STATE.get_or_init(|| {
            env_logger::init();

            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

            let adapter = pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
            )
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

            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::IMMEDIATES | wgpu::Features::SUBGROUP,
                    required_limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                }))
                .expect("Failed to create device");

            State { device, queue }
        })
    }
}
