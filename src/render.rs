use crate::Vec2U32;

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Immediates {
    pub size: Vec2U32,
    pub image_size: Vec2U32,
}
