#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, atomic::AtomicBool};

use eframe::{egui, egui_wgpu};
use pixel_sort::{PixelSort, U32_SIZE, U64_SIZE, Vec2U32, const_max_u32_slice, const_size_of_u32};
use wgpu::util::DeviceExt;

fn main() -> eframe::Result {
    env_logger::init();

    let mut native_options = eframe::NativeOptions::default();
    match &mut native_options.wgpu_options.wgpu_setup {
        egui_wgpu::WgpuSetup::CreateNew(wgpu_setup_create_new) => {
            let previous = Arc::clone(&wgpu_setup_create_new.device_descriptor);
            wgpu_setup_create_new.device_descriptor = Arc::new(move |adapter| {
                let mut device_descriptor = previous(adapter);

                device_descriptor.required_features |= REQUIRED_FEATURES;
                device_descriptor.required_limits.max_immediate_size = device_descriptor
                    .required_limits
                    .max_immediate_size
                    .max(REQUIRED_IMMEDIATE_SIZE);
                device_descriptor
                    .required_limits
                    .max_storage_buffer_binding_size = REQUIRED_STORAGE_BUFFER_BINDING_SIZE;
                device_descriptor.required_limits.max_buffer_size =
                    REQUIRED_STORAGE_BUFFER_BINDING_SIZE;

                device_descriptor
            })
        }
        egui_wgpu::WgpuSetup::Existing(wgpu_setup_existing) => {
            let device = &wgpu_setup_existing.device;

            assert!(device.features().contains(REQUIRED_FEATURES));
            assert!(device.limits().max_immediate_size >= REQUIRED_IMMEDIATE_SIZE);
        }
    }

    eframe::run_native(
        "Pixel Sort",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct App {
    first: bool,
    threshold: f32,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &wgpu_render_state.device;

        let img = image::ImageReader::open("examples/source.jpg")
            .unwrap()
            .decode()
            .unwrap()
            .into_rgba8();
        let image = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &img,
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        });
        let image_size = Vec2U32 {
            x: img.width(),
            y: img.height(),
        };

        let workgroup_size = wgpu_render_state.adapter.get_info().subgroup_max_size;

        let sort = PixelSort::new(device, workgroup_size, MAX_IMAGE_PIXELS);

        let render_module = device.create_shader_module(wgpu::include_wgsl!("app_viewer.wgsl"));

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
            resource: sort.image.as_entire_binding(),
        };

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[image_layout],
            });
        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &render_bind_group_layout,
            entries: &[image_entry],
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&render_bind_group_layout)],
                immediate_size: const { const_size_of_u32::<ViewerImmediates>() },
            });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&render_pipeline_layout),
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
                targets: &[Some(wgpu_render_state.target_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        wgpu_render_state
            .renderer
            .write()
            .callback_resources
            .insert(ViewerResources {
                image,
                image_size,
                sort,
                render_bind_group,
                render_pipeline,
            });

        Self {
            first: true,
            threshold: 0.5,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let mut updated = false;

                ui.vertical(|ui| {
                    ui.heading("Pixel Sort");
                    let slider = ui.add(egui::Slider::new(&mut self.threshold, 0.0..=1.0));
                    updated = slider.changed();
                });

                if self.first {
                    self.first = false;
                    updated = true;
                }

                ui.add(Viewer {
                    size: ui.available_size(),
                    updated: AtomicBool::new(updated),
                    threshold: self.threshold,
                });
            })
        });
    }
}

struct Viewer {
    size: egui::Vec2,
    updated: AtomicBool,
    threshold: f32,
}

impl egui::Widget for Viewer {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        egui::Frame::canvas(ui.style())
            .show(ui, |ui| {
                let (_id, rect) = ui.allocate_space(self.size);
                ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                    rect,
                    ViewerCallback { viewer: self },
                ));
            })
            .response
    }
}

struct ViewerCallback {
    viewer: Viewer,
}

impl egui_wgpu::CallbackTrait for ViewerCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Viewer {
            size,
            updated,
            threshold,
        } = &self.viewer;
        if !updated.swap(false, std::sync::atomic::Ordering::AcqRel) {
            return Vec::new();
        }

        let ViewerResources {
            image,
            image_size,
            sort,
            ..
        } = callback_resources.get::<ViewerResources>().unwrap();

        sort.add_step(egui_encoder, *image_size, *threshold, image);

        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let ViewerResources {
            image_size,
            render_bind_group,
            render_pipeline,
            ..
        } = callback_resources.get::<ViewerResources>().unwrap();

        let viewport = info.viewport_in_pixels();

        render_pass.set_pipeline(render_pipeline);
        render_pass.set_bind_group(0, render_bind_group, &[]);
        render_pass.set_immediates(
            0,
            bytemuck::bytes_of(&ViewerImmediates {
                viewport_top_left: [viewport.left_px as f32, viewport.top_px as f32],
                viewport_size: [viewport.width_px as f32, viewport.height_px as f32],
                image_size: *image_size,
            }),
        );
        render_pass.draw(0..6, 0..1);
    }
}

struct ViewerResources {
    image: wgpu::Buffer,
    image_size: Vec2U32,
    sort: PixelSort,
    render_bind_group: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
struct ViewerImmediates {
    viewport_top_left: [f32; 2],
    viewport_size: [f32; 2],
    image_size: Vec2U32,
}

const REQUIRED_FEATURES: wgpu::Features =
    wgpu::Features::IMMEDIATES.union(wgpu::Features::SUBGROUP);
const REQUIRED_IMMEDIATE_SIZE: u32 = const_max_u32_slice(&[
    pixel_sort::IMMEDIATES_SIZE,
    const_size_of_u32::<ViewerImmediates>(),
]);
const MAX_IMAGE_PIXELS: u64 = 7680 * 4320; // 8k
const REQUIRED_STORAGE_BUFFER_BINDING_SIZE: u64 = MAX_IMAGE_PIXELS * U64_SIZE.get() * 2;
