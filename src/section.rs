use crate::{
    Vec2U32, WorkgroupInfo, section_down::SectionDown, section_global::SectionGlobal,
    section_up::SectionUp,
};

pub struct Section {
    pub up: SectionUp,
    pub global: SectionGlobal,
    pub down: SectionDown,
}

impl Section {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        max_image_size: Vec2U32,
    ) -> Self {
        let up = SectionUp::new(device, workgroup_info, max_image_size);
        let global =
            SectionGlobal::new(device, workgroup_info, &up.left_workgroup, max_image_size.y);
        let down = SectionDown::new(
            device,
            workgroup_info,
            max_image_size,
            &up.image,
            &up.left_workgroup,
            &up.workgroup_right,
        );

        Self { up, global, down }
    }

    pub fn add_step(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image_size: Vec2U32,
        threshold: f32,
        image: &wgpu::Buffer,
    ) {
        let workgroups = self.up.add_step(encoder, image_size, threshold, image);
        self.global.add_step(encoder, image_size.y, workgroups);
        self.down.add_step(encoder, image_size, threshold);
    }

    #[must_use]
    #[inline]
    pub fn tagged_image(&self) -> &wgpu::Buffer {
        &self.down.tagged_image
    }
}
