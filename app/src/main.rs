#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    ffi::{OsStr, OsString},
    num::NonZero,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use eframe::{egui, egui_wgpu};
use pixel_sort::{
    Vec2U32, add_requred_features_and_limits,
    utils::{U32_SIZE, const_max_u32_slice, const_size_of_u32},
};

mod settings;
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
    settings: Vec<(settings::Settings, bool)>,
    #[cfg(feature = "screenshot")]
    show_screenshot_button: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();
        let device = &wgpu_render_state.device;

        let current_image = std::path::absolute("examples/source.jpg").unwrap();
        let (img, image_size) = read_image(&current_image);

        let viewer_resources = ViewerResources::new(device, wgpu_render_state.target_format);

        wgpu_render_state
            .renderer
            .write()
            .callback_resources
            .insert(viewer_resources);

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
            settings: vec![(settings::Settings::new(), false)],
            #[cfg(feature = "screenshot")]
            show_screenshot_button: true,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let mut image_update = None;
                let mut config_updated = false;
                let mut vertical_updated = false;

                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Pixel Sort");

                        #[cfg(feature = "screenshot")]
                        {
                            if std::mem::replace(&mut self.show_screenshot_button, false)
                                && ui.button("Take screenshot").clicked()
                            {
                                ui.ctx().request_screenshot(|screenshot| {
                                    let Some(path) = rfd::FileDialog::new()
                                        .add_filter(
                                            "image",
                                            &image::ImageFormat::all()
                                                .filter(image::ImageFormat::writing_enabled)
                                                .flat_map(image::ImageFormat::extensions_str)
                                                .collect::<Vec<_>>(),
                                        )
                                        .set_directory(std::env::current_dir().unwrap())
                                        .save_file()
                                    else {
                                        return;
                                    };

                                    image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                                        u32::try_from(screenshot.width()).unwrap(),
                                        u32::try_from(screenshot.height()).unwrap(),
                                        screenshot.as_raw(),
                                    )
                                    .unwrap()
                                    .save(path)
                                    .unwrap();
                                });
                            }
                        }

                        image_update = self.open_image_ui(ui);
                        self.save_image_ui(ui, frame.wgpu_render_state().unwrap());

                        config_updated |= self.add_config_ui(ui, 0);

                        let mut i = 0;
                        while let Some(response) = self.config_ui(ui, i) {
                            i += 1;

                            config_updated |= response.config_updated;
                            vertical_updated |= response.vertical_updated;
                            if response.removed {
                                i -= 1;
                            }

                            config_updated |= self.add_config_ui(ui, i);
                        }
                    })
                });

                if let Some(initial_image) = self.initial_image.take() {
                    image_update = Some(image_update.unwrap_or(initial_image));
                    config_updated = true;
                }

                let config = config_updated.then(|| {
                    self.settings
                        .iter()
                        .map(|(settings, _)| settings.get_config())
                        .collect()
                });

                let mut vertical = [false; Viewer::CAPACITY];
                vertical
                    .iter_mut()
                    .zip(self.settings.iter().map(|(_, vertical)| vertical))
                    .for_each(|(dst, src)| *dst = *src);

                ui.add(Viewer {
                    size: ui.available_size(),
                    image_size: self.image_size,
                    image_updated: AtomicBool::new(image_update.is_some()),
                    image: image_update,
                    num_configs: self.settings.len() as u32,
                    config,
                    config_updated: AtomicBool::new(config_updated),
                    vertical,
                    vertical_updated: AtomicBool::new(vertical_updated),
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

impl App {
    #[must_use]
    fn open_image_ui(
        &mut self,
        ui: &mut egui::Ui,
    ) -> Option<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
        ui.button("Open image file")
            .clicked()
            .then(|| {
                rfd::FileDialog::new()
                    .add_filter("image", &self.open_image_extensions)
                    .set_directory(self.current_image.parent().unwrap())
                    .pick_file()
            })
            .flatten()
            .map(|path| {
                let (img, image_size) = read_image(&path);
                self.image_size = image_size;
                self.current_image = path;
                img
            })
    }

    fn save_image_ui(&self, ui: &mut egui::Ui, render_state: &egui_wgpu::RenderState) {
        if ui.button("Save image").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("image", &self.save_image_extensions)
                .set_directory(self.current_image.parent().unwrap())
                .set_file_name({
                    let mut file_name = PathBuf::from(self.current_image.file_name().unwrap());
                    file_name.set_extension("");
                    let mut file_name = OsString::from(file_name);
                    file_name.push(" pixel-sorted");
                    let mut file_name = PathBuf::from(file_name);
                    file_name
                        .set_extension(self.current_image.extension().unwrap_or(OsStr::new("")));

                    file_name.into_os_string().into_string().unwrap()
                })
                .save_file()
        {
            download_write_image(render_state, self.image_size, &path);
        }
    }

    #[must_use]
    fn add_config_ui(&mut self, ui: &mut egui::Ui, position: usize) -> bool {
        let clicked = ui
            .add_enabled(
                self.settings.len() < Viewer::CAPACITY,
                egui::Button::new("+"),
            )
            .clicked();
        if clicked {
            self.settings
                .insert(position, (settings::Settings::new(), false));
        }
        clicked
    }

    #[must_use]
    fn config_ui(&mut self, ui: &mut egui::Ui, i: usize) -> Option<ConfigUiResponse> {
        let (settings, vertical) = self.settings.get_mut(i)?;

        let mut response = ConfigUiResponse {
            config_updated: false,
            vertical_updated: false,
            removed: false,
        };

        if egui::Frame::group(ui.style())
            .show(ui, |ui| {
                let delete_response = ui
                    .horizontal(|ui| {
                        response.vertical_updated = ui.checkbox(vertical, "Vertical").changed();
                        ui.button("×")
                    })
                    .inner;
                response.config_updated = ui.add(settings.widget()).changed();

                delete_response
            })
            .inner
            .clicked()
        {
            response.removed = true;
            self.settings.remove(i);
            response.config_updated = true;
        }

        Some(response)
    }
}

struct ConfigUiResponse {
    config_updated: bool,
    vertical_updated: bool,
    removed: bool,
}
