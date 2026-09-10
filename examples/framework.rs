use std::sync::Arc;

use pixel_sort::{U32_SIZE, Vec2U32, const_max_u32_slice, const_size_of_u32};
use wgpu::util::DeviceExt;
use winit::window::Window;

#[allow(dead_code)]
fn main() {
    eprintln!("This is a framework module for the other examples!");
    eprintln!("Try 'regions' or 'sort'");
}

pub trait Example: Sized {
    type Immediates: bytemuck::Pod;

    #[expect(unused_variables)]
    fn get_max_buffer_size(image_size: Vec2U32) -> u64 {
        0
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
    );
    fn immediates(&self, window_size: Vec2U32) -> Self::Immediates;
}

pub fn run<E: Example>() {
    env_logger::init();

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::<E> { state: None };
    event_loop.run_app(&mut app).unwrap();
}

struct App<E> {
    state: Option<State<E>>,
}

struct State<E> {
    window: Arc<winit::window::Window>,
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    example: E,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl<E: Example> winit::application::ApplicationHandler for App<E> {
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
        _window_id: winit::window::WindowId,
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
                device_id: _,
                event,
                is_synthetic: _,
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

impl<E: Example> State<E> {
    fn new(
        display: winit::event_loop::OwnedDisplayHandle,
        window: Arc<winit::window::Window>,
    ) -> Self {
        let img = image::ImageReader::open("examples/source.jpg")
            .unwrap()
            .decode()
            .unwrap()
            .into_rgba8();
        let image_size = Vec2U32 {
            x: img.width(),
            y: img.height(),
        };

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
        required_limits.max_immediate_size = const {
            const_max_u32_slice(&[
                pixel_sort::IMMEDIATES_SIZE,
                const_size_of_u32::<E::Immediates>(),
            ])
        };
        let max_buffer_size = (Vec2U32 {
            x: img.width(),
            y: img.height(),
        }
        .product()
            * U32_SIZE.get())
        .max(E::get_max_buffer_size(image_size));
        required_limits.max_storage_buffer_binding_size = max_buffer_size;
        required_limits.max_buffer_size = max_buffer_size;

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

        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &img,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        });

        let (example, module, bind_group_layout, bind_group) = E::new(
            &device,
            &queue,
            adapter.get_info().subgroup_max_size,
            image,
            image_size,
        );

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: const { const_size_of_u32::<E::Immediates>() },
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(surface_format.add_srgb_suffix().into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            window,
            instance,
            device,
            queue,
            size,
            surface,
            surface_format,
            example,
            bind_group,
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
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_immediates(
            0,
            bytemuck::bytes_of(&self.example.immediates(Vec2U32 {
                x: self.size.width,
                y: self.size.height,
            })),
        );
        render_pass.draw(0..6, 0..1);
        drop(render_pass);

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        self.queue.present(surface_texture);
    }
}
