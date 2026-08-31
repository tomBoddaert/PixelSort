use std::sync::Arc;

use pixel_sort::{
    IMMEDIATES_SIZE, U64_SIZE, Vec2U32, WorkgroupInfo, const_size_of_u32,
    section_down::SectionDown, section_global::SectionGlobal, section_up::SectionUp,
};
use wgpu::util::DeviceExt;
use winit::window::Window;

fn main() {
    env_logger::init();

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}

#[derive(Default)]
struct App {
    state: Option<State>,
}

struct State {
    window: Arc<winit::window::Window>,
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    image_size: Vec2U32,
    render_bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl winit::application::ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );

        self.state = Some(State::new(event_loop.owned_display_handle(), window));
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        match event {
            winit::event::WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            winit::event::WindowEvent::RedrawRequested => {
                state.draw();
                state.window.request_redraw();
            }
            winit::event::WindowEvent::Resized(size) => {
                // Reconfigures the size of the surface. We do not re-render
                // here as this event is always followed up by redraw request.
                state.resize(size);
            }

            winit::event::WindowEvent::KeyboardInput {
                device_id,
                event,
                is_synthetic,
            } => match event.logical_key {
                winit::keyboard::Key::Character(c) if c == "q" => {
                    event_loop.exit();
                }
                _ => {}
            },
            _ => (),
        }
    }
}

impl State {
    fn new(
        display: winit::event_loop::OwnedDisplayHandle,
        window: Arc<winit::window::Window>,
    ) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(display),
        ));

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

        let size = window.inner_size();

        let surface = instance.create_surface(Arc::clone(&window)).unwrap();
        let surface_capabilities = surface.get_capabilities(&adapter);
        let surface_format = surface_capabilities.formats[0];

        let workgroup_info = WorkgroupInfo {
            workgroup_size: adapter.get_info().subgroup_max_size,
            max_workgroups: 64,
            // max_workgroups: 2,
        };

        let img = image::ImageReader::open("source.jpg")
            .unwrap()
            .decode()
            .unwrap();
        let img = img.into_rgba8();
        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &img,
            usage: wgpu::BufferUsages::STORAGE,
        });
        let image_size = Vec2U32 {
            x: img.width(),
            y: img.height(),
        };

        let section_up = SectionUp::new(&device, workgroup_info, image_size, &image);
        let section_global = SectionGlobal::new(
            &device,
            workgroup_info,
            &section_up.left_workgroup,
            image_size.y,
        );
        let section_down = SectionDown::new(
            &device,
            workgroup_info,
            image_size,
            &image,
            &section_up.left_workgroup,
            &section_up.workgroup_right,
        );

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
            resource: section_down.tagged_image.as_entire_binding(),
        };

        let render_module = device.create_shader_module(wgpu::include_wgsl!("regions.wgsl"));

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[tagged_image_layout],
            });
        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &render_bind_group_layout,
            entries: &[tagged_image_entry],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&render_bind_group_layout)],
            immediate_size: const { const_size_of_u32::<Immediates>() },
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_module,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &render_module,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(surface_format.add_srgb_suffix().into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let workgroups = section_up.add_step(&mut encoder);
        section_global.add_step(&mut encoder, workgroups);
        section_down.add_step(&mut encoder);
        let idx = queue.submit([encoder.finish()]);

        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(idx),
                timeout: None,
            })
            .unwrap();

        Self {
            window,
            instance,
            device,
            queue,
            size,
            surface,
            surface_format,
            image_size,
            render_bind_group,
            pipeline,
        }
    }

    fn configure_surface(&self) {
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: self.size.width,
            height: self.size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![self.surface_format.add_srgb_suffix()],
        };
        self.surface.configure(&self.device, &surface_config);
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;
        self.configure_surface();
    }

    fn get_surface_texture(&mut self) -> Option<wgpu::SurfaceTexture> {
        // Create texture view.
        // NOTE: We must handle Timeout because the surface may be unavailable
        // (e.g., when the window is occluded on macOS).
        Some(match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return None;
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                drop(texture);
                self.configure_surface();
                return None;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure_surface();
                return None;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                unreachable!("No error scope registered, so validation errors will panic")
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(self.window.clone()).unwrap();
                self.configure_surface();
                return None;
            }
        })
    }

    fn get_texture_view(&self, surface_texture: &wgpu::SurfaceTexture) -> wgpu::TextureView {
        surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                // Without add_srgb_suffix() the image we will be working with
                // might not be "gamma correct".
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            })
    }

    fn draw(&mut self) {
        let Some(surface_texture) = self.get_surface_texture() else {
            return;
        };
        let texture_view = self.get_texture_view(&surface_texture);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.render_bind_group, &[]);
        render_pass.set_immediates(
            0,
            bytemuck::bytes_of(&Immediates {
                size: Vec2U32 {
                    x: self.size.width,
                    y: self.size.height,
                },
                image_size: self.image_size,
            }),
        );
        render_pass.draw(0..6, 0..1);
        drop(render_pass);

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub size: Vec2U32,
    pub image_size: Vec2U32,
}
