use crate::{
    BIT_LEN, U64_SIZE, Vec2U32, WorkgroupInfo, const_u32_to_usize, count::Count,
    partial_sum::PartialSum, reorder::Reorder,
};

pub struct Sort {
    pub count: Count,
    pub partial_sum: PartialSum,
    pub reorder: Reorder,
}

impl Sort {
    pub fn new(
        device: &wgpu::Device,
        workgroup_info: WorkgroupInfo,
        max_image_size: Vec2U32,
        tagged_image: &wgpu::Buffer,
    ) -> Self {
        let count = Count::new(device, workgroup_info, max_image_size, tagged_image);
        let partial_sum = PartialSum::new(device, workgroup_info, &count.counts, max_image_size.y);
        let reorder = Reorder::new(
            device,
            workgroup_info,
            tagged_image,
            &partial_sum.offsets,
            max_image_size,
        );

        Self {
            count,
            partial_sum,
            reorder,
        }
    }

    pub fn add_single_step(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        image_size: Vec2U32,
        bit_offset: u32,
    ) {
        let workgroups = self.count.add_step(encoder, image_size, bit_offset);
        self.partial_sum.add_step(encoder, image_size.y, workgroups);
        self.reorder.add_step(encoder, image_size, bit_offset);
        encoder.copy_buffer_to_buffer(
            &self.reorder.output,
            0,
            &self.count.tagged_image,
            0,
            image_size.product() * U64_SIZE.get(),
        );
    }

    pub fn add_step(&self, encoder: &mut wgpu::CommandEncoder, image_size: Vec2U32) {
        for bit_offset in [0..u8::BITS, 16..(16 + u16::BITS)]
            .into_iter()
            .flat_map(|bits| bits.step_by(const { const_u32_to_usize(BIT_LEN) }))
        {
            self.add_single_step(encoder, image_size, bit_offset);
        }
    }

    #[must_use]
    #[inline]
    pub fn tagged_image(&self) -> &wgpu::Buffer {
        &self.reorder.tagged_image
    }
}
