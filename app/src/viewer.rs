use std::sync::atomic::AtomicBool;

use eframe::{egui, egui_wgpu};
use pixel_sort::{PixelSort, Vec2U32, config::Config, utils::U32_SIZE};

pub struct Viewer {
    pub size: egui::Vec2,
    pub image_size: Vec2U32,
    pub image: Option<image::RgbaImage>,
    pub image_updated: AtomicBool,
    pub config: Option<Config>,
    pub config_updated: AtomicBool,
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
        _queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Viewer {
            image_size,
            image,
            image_updated,
            config,
            config_updated,
            ..
        } = &self.viewer;
        let config_update = config_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        let image_update = image_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        if !(config_update || image_update) {
            return Vec::new();
        }

        let ViewerResources {
            image_upload,
            config_upload,
            sort,
            ..
        } = callback_resources.get::<ViewerResources>().unwrap();

        if image_update && let Some(image) = image {
            let image_byte_len = Vec2U32 {
                x: image.width(),
                y: image.height(),
            }
            .product()
                * U32_SIZE.get();
            let mut mapped = image_upload.get_mapped_range_mut(..image_byte_len).unwrap();
            mapped.copy_from_slice(bytemuck::cast_slice(image));
            drop(mapped);
            image_upload.unmap();

            sort.copy_to_input(egui_encoder, image_upload, *image_size)
                .unwrap();
            egui_encoder.map_buffer_on_submit(image_upload, wgpu::MapMode::Write, .., |_| {});
        }

        if config_update && let Some(config) = config {
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

            let mut mapped = config_upload.get_mapped_range_mut(..).unwrap();
            mapped.copy_from_slice(bytemuck::bytes_of(config));
            drop(mapped);
            config_upload.unmap();

            sort.copy_to_config(egui_encoder, config_upload, 0).unwrap();
            egui_encoder.map_buffer_on_submit(config_upload, wgpu::MapMode::Write, .., |_| {});
        }

        sort.add_step(egui_encoder, *image_size, false).unwrap();

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

pub struct ViewerResources {
    pub image_upload: wgpu::Buffer,
    pub config_upload: wgpu::Buffer,
    pub download: wgpu::Buffer,
    pub sort: PixelSort,
    pub render_bind_group: wgpu::BindGroup,
    pub render_pipeline: wgpu::RenderPipeline,
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct ViewerImmediates {
    pub viewport_top_left: [f32; 2],
    pub viewport_size: [f32; 2],
    pub image_size: Vec2U32,
}
