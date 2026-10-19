#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::{OsStr, OsString},
    num::NonZero,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use eframe::{egui, egui_wgpu};
use pixel_sort::{
    PixelSort, Vec2U32, add_requred_features_and_limits,
    config::{self, CONFIG_SIZE},
    utils::{U32_SIZE, const_max_u32_slice, const_size_of_u32},
};

mod viewer;
use viewer::Viewer;

use crate::viewer::{ViewerImmediates, ViewerResources};

fn main() -> eframe::Result {
    env_logger::init();

    let mut native_options = eframe::NativeOptions::default();
    match &mut native_options.wgpu_options.wgpu_setup {
        egui_wgpu::WgpuSetup::CreateNew(wgpu_setup_create_new) => {
            let previous = Arc::clone(&wgpu_setup_create_new.device_descriptor);
            wgpu_setup_create_new.device_descriptor = Arc::new(move |adapter| {
                let device_descriptor = previous(adapter);
                let mut device_descriptor =
                    add_requred_features_and_limits(device_descriptor, MAX_IMAGE_PIXELS).unwrap();

                device_descriptor.required_limits.max_immediate_size = device_descriptor
                    .required_limits
                    .max_immediate_size
                    .max(REQUIRED_IMMEDIATE_SIZE);

                device_descriptor
            });
        }
        egui_wgpu::WgpuSetup::Existing(_wgpu_setup_existing) => {
            unreachable!();
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

        let image_upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image_upload"),
            size: const { MAX_IMAGE_PIXELS.get() * U32_SIZE.get() },
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        let config_upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("config_upload"),
            size: CONFIG_SIZE.get(),
            usage: image_upload.usage(),
            mapped_at_creation: true,
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("download"),
            size: image_upload.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let workgroup_size = wgpu_render_state.adapter.get_info().subgroup_max_size;

        let sort = PixelSort::new(device, workgroup_size, MAX_IMAGE_PIXELS).unwrap();

        let render_module = device.create_shader_module(wgpu::include_wgsl!("render.wgsl"));

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
                image_upload,
                config_upload,
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
                    threshold_updated = true;
                }

                let config = threshold_updated.then(|| {
                    config::ConfigBuffer::single_sorted(
                        config::Sort::Increasing,
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "self.threshold is in 0..=1 and must be converted to a u8"
                        )]
                        {
                            (self.threshold * const { u8::MAX as f32 }) as u8
                        },
                        config::Sort::Increasing,
                    )
                    .finish()
                });

                ui.add(Viewer {
                    size: ui.available_size(),
                    image_size: self.image_size,
                    image_updated: AtomicBool::new(image_update.is_some()),
                    image: image_update,
                    config,
                    config_updated: AtomicBool::new(config.is_some()),
                });
            })
        });
    }
}

const REQUIRED_IMMEDIATE_SIZE: u32 = const_max_u32_slice(&[
    pixel_sort::IMMEDIATES_SIZE,
    const_size_of_u32::<ViewerImmediates>(),
]);
const MAX_IMAGE_PIXELS: NonZero<u64> = NonZero::new(7680 * 4320).unwrap(); // 8k

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
        .copy_from_output(&mut encoder, &resources.download, image_size)
        .unwrap();
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
    drop(renderer); // Purely to tighten the drop of renderer
    let rgba = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(
        image_size.x,
        image_size.y,
        &downloaded,
    )
    .unwrap();
    let mut rgb = image::RgbImage::new(image_size.x, image_size.y);
    rgb.copy_from_color_space(&rgba, image::ConvertColorOptions::default())
        .unwrap();

    rgb.save(path).unwrap();
}
