use std::sync::atomic::AtomicBool;

use eframe::{egui, egui_wgpu};
use pixel_sort::{
    PixelSort, Vec2U32,
    config::{CONFIG_SIZE, Config},
    utils::{U32_SIZE, const_size_of_u32},
};

use crate::MAX_IMAGE_PIXELS;

pub struct Viewer {
    pub size: egui::Vec2,
    pub image_size: Vec2U32,
    pub image: Option<image::RgbaImage>,
    pub image_updated: AtomicBool,
    pub num_configs: u32,
    pub config: Option<Vec<Config>>,
    pub config_updated: AtomicBool,
    pub vertical: [bool; Self::CAPACITY],
    pub vertical_updated: AtomicBool,
}

impl Viewer {
    pub const CAPACITY_U16: u16 = 8;
    pub const CAPACITY: usize = Self::CAPACITY_U16 as usize;
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
            num_configs,
            config,
            config_updated,
            vertical,
            vertical_updated,
            ..
        } = &self.viewer;
        let image_update = image_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        let config_update = config_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        let vertical_update = vertical_updated.swap(false, std::sync::atomic::Ordering::AcqRel);
        if !(image_update || config_update || vertical_update) {
            return Vec::new();
        }

        let ViewerResources {
            image_upload,
            image: image_buffer,
            config_upload,
            configs,
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

            egui_encoder.copy_buffer_to_buffer(
                image_upload,
                0,
                image_buffer,
                0,
                image_size.product() * U32_SIZE.get(),
            );
            egui_encoder.map_buffer_on_submit(image_upload, wgpu::MapMode::Write, .., |_| {});
        }

        if config_update && let Some(config) = config {
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

            let configs_size = pixel_sort::config::CONFIG_SIZE.get() * config.len() as u64;
            let mut mapped = config_upload.get_mapped_range_mut(..configs_size).unwrap();
            mapped.copy_from_slice(bytemuck::cast_slice(config));
            drop(mapped);
            config_upload.unmap();

            egui_encoder.copy_buffer_to_buffer(config_upload, 0, configs, 0, configs_size);
            egui_encoder.map_buffer_on_submit(config_upload, wgpu::MapMode::Write, .., |_| {});
        }

        sort.copy_to_input(egui_encoder, image_buffer, *image_size)
            .unwrap();
        for (i, vertical) in (0..*num_configs).zip(vertical) {
            sort.copy_to_config(egui_encoder, configs, i).unwrap();
            sort.add_step(egui_encoder, *image_size, *vertical).unwrap();
            sort.copy_output_to_input(egui_encoder, *image_size)
                .unwrap();
        }

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
    pub image: wgpu::Buffer,
    pub config_upload: wgpu::Buffer,
    pub configs: wgpu::Buffer,
    pub download: wgpu::Buffer,
    pub sort: PixelSort,
    pub render_bind_group: wgpu::BindGroup,
    pub render_pipeline: wgpu::RenderPipeline,
}

impl ViewerResources {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let image_upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image_upload"),
            size: const { MAX_IMAGE_PIXELS.get() * U32_SIZE.get() },
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        let image = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image"),
            size: image_upload.size(),
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let config_upload = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("config_upload"),
            size: const { CONFIG_SIZE.get() * Viewer::CAPACITY_U16 as u64 },
            usage: image_upload.usage(),
            mapped_at_creation: true,
        });
        let configs = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("configs"),
            size: config_upload.size(),
            usage: image.usage(),
            mapped_at_creation: false,
        });
        let download = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("download"),
            size: image_upload.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let workgroup_size = device.adapter_info().subgroup_max_size;

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
            resource: sort.input.as_entire_binding(),
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
                targets: &[Some(target_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            image_upload,
            image,
            config_upload,
            configs,
            download,
            sort,
            render_bind_group,
            render_pipeline,
        }
    }
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct ViewerImmediates {
    pub viewport_top_left: [f32; 2],
    pub viewport_size: [f32; 2],
    pub image_size: Vec2U32,
}
