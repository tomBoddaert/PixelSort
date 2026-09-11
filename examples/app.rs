#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use eframe::{egui, egui_wgpu};
use pixel_sort::{PixelSort, U32_SIZE, U64_SIZE, Vec2U32, const_max_u32_slice, const_size_of_u32};

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
    open_image_extensions: Vec<&'static str>,
    save_image_extensions: Vec<&'static str>,
    initial_image: Option<image::RgbaImage>,
    current_image: PathBuf,
    image_size: Vec2U32,
    threshold: f32,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &wgpu_render_state.device;

        let current_image = std::path::absolute("examples/source.jpg").unwrap();
        let (img, image_size) = read_image(&current_image);

        let upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upload"),
            size: const { MAX_IMAGE_PIXELS * U32_SIZE.get() },
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("download"),
            size: upload.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

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
            resource: sort.output.as_entire_binding(),
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
                upload,
                download,
                sort,
                render_bind_group,
                render_pipeline,
            });

        Self {
            open_image_extensions: image::ImageFormat::all()
                .filter(image::ImageFormat::reading_enabled)
                .flat_map(image::ImageFormat::extensions_str)
                .copied()
                .collect(),
            save_image_extensions: image::ImageFormat::all()
                .filter(image::ImageFormat::writing_enabled)
                .flat_map(image::ImageFormat::extensions_str)
                .copied()
                .collect(),
            initial_image: Some(img),
            current_image,
            image_size,
            threshold: 0.5,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let mut image_update = None;
                let mut threshold_updated = false;

                ui.vertical(|ui| {
                    ui.heading("Pixel Sort");
                    let slider = ui.add(egui::Slider::new(&mut self.threshold, 0.0..=1.0));
                    threshold_updated = slider.changed();

                    if ui.button("Open image file").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("image", &self.open_image_extensions)
                            .set_directory(self.current_image.parent().unwrap())
                            .pick_file()
                    {
                        let (img, image_size) = read_image(&path);
                        self.image_size = image_size;
                        self.current_image = path;
                        image_update = Some(img);
                    }

                    if ui.button("Save image").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("image", &self.save_image_extensions)
                            .set_directory(self.current_image.parent().unwrap())
                            .set_file_name({
                                let mut file_name =
                                    PathBuf::from(self.current_image.file_name().unwrap());
                                file_name.set_extension("");
                                let mut file_name = OsString::from(file_name);
                                file_name.push(" pixel-sorted");
                                let mut file_name = PathBuf::from(file_name);
                                file_name.set_extension(
                                    self.current_image.extension().unwrap_or(OsStr::new("")),
                                );

                                file_name.into_os_string().into_string().unwrap()
                            })
                            .save_file()
                    {
                        let render_state = frame.wgpu_render_state().unwrap();
                        download_write_image(render_state, self.image_size, &path);
                    }
                });

                if let Some(initial_image) = self.initial_image.take() {
                    image_update = Some(image_update.unwrap_or(initial_image));
                }

                ui.add(Viewer {
                    size: ui.available_size(),
                    image_size: self.image_size,
                    image_updated: AtomicBool::new(image_update.is_some()),
                    image: image_update,
                    threshold: self.threshold,
                    threshold_updated: AtomicBool::new(threshold_updated),
                });
            })
        });
    }
}

struct Viewer {
    size: egui::Vec2,
    image_size: Vec2U32,
    image: Option<image::RgbaImage>,
    image_updated: AtomicBool,
    threshold: f32,
    threshold_updated: AtomicBool,
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
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Viewer {
            image_size,
            image,
            image_updated,
            threshold,
            threshold_updated,
            ..
        } = &self.viewer;
        let threshold_update = threshold_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        let image_update = image_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        if !(threshold_update || image_update) {
            return Vec::new();
        }

        let ViewerResources { upload, sort, .. } =
            callback_resources.get::<ViewerResources>().unwrap();

        if image_update && let Some(image) = image {
            let image_byte_len = Vec2U32 {
                x: image.width(),
                y: image.height(),
            }
            .product()
                * U32_SIZE.get();
            let mut mapped = upload.get_mapped_range_mut(..image_byte_len).unwrap();
            mapped.copy_from_slice(bytemuck::cast_slice(image));
            drop(mapped);
            upload.unmap();

            sort.copy_to_input(egui_encoder, upload, *image_size);
            egui_encoder.map_buffer_on_submit(upload, wgpu::MapMode::Write, .., |_| {});
        }

        sort.add_step(egui_encoder, *image_size, *threshold);

        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let ViewerResources {
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
                image_size: self.viewer.image_size,
            }),
        );
        render_pass.draw(0..6, 0..1);
    }
}

struct ViewerResources {
    upload: wgpu::Buffer,
    download: wgpu::Buffer,
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

fn read_image(path: &Path) -> (image::RgbaImage, Vec2U32) {
    let img = image::ImageReader::open(path)
        .unwrap()
        .decode()
        .unwrap()
        .into_rgba8();
    let image_size = Vec2U32 {
        x: img.width(),
        y: img.height(),
    };

    (img, image_size)
}

fn download_write_image(render_state: &egui_wgpu::RenderState, image_size: Vec2U32, path: &Path) {
    let renderer = render_state.renderer.read();
    let resources = renderer
        .callback_resources
        .get::<ViewerResources>()
        .unwrap();

    let mut encoder = render_state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("download encoder"),
        });
    resources
        .sort
        .copy_from_output(&mut encoder, &resources.download, image_size);
    let bounds = ..image_size.product() * U32_SIZE.get();
    encoder.map_buffer_on_submit(&resources.download, wgpu::MapMode::Read, bounds, |_| {});
    let ix = render_state.queue.submit([encoder.finish()]);
    render_state
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(ix),
            timeout: None,
        })
        .unwrap();

    let downloaded = resources.download.get_mapped_range(bounds).unwrap();
    let rgba = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
        image_size.x,
        image_size.y,
        &downloaded,
    )
    .unwrap();
    let mut rgb = image::RgbImage::new(image_size.x, image_size.y);
    rgb.copy_from_color_space(&rgba, image::ConvertColorOptions::default())
        .unwrap();

    image::save_buffer(
        path,
        &rgb,
        image_size.x,
        image_size.y,
        image::ColorType::Rgb8,
    )
    .unwrap();
}
