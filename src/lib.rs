use std::num::NonZero;

pub mod count;
pub mod partial_sum;
pub mod reorder;
pub mod section;
pub mod section_down;
pub mod section_global;
pub mod section_up;
pub mod sort;

pub const U32_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u32>())).unwrap();
pub const U64_SIZE: NonZero<u64> = NonZero::new(const_usize_to_u64(size_of::<u64>())).unwrap();
pub const BIT_LEN: u32 = 4;
pub const BASE: u32 = 2_u32.pow(BIT_LEN);

// TODO: replace this with a wgsl module to copy buffers without COPY_SRC for testing
#[cfg(not(test))]
const TEST_COPY_SRC: wgpu::BufferUsages = wgpu::BufferUsages::empty();
#[cfg(test)]
const TEST_COPY_SRC: wgpu::BufferUsages = wgpu::BufferUsages::COPY_SRC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkgroupInfo {
    pub workgroup_size: u32,
    pub max_workgroups: u32,
}

#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct Vec2U32 {
    pub x: u32,
    pub y: u32,
}
impl Vec2U32 {
    #[must_use]
    #[inline]
    pub fn product(self) -> u64 {
        u64::from(self.x) * u64::from(self.y)
    }
}

pub const IMMEDIATES_SIZE: u32 = const_max_u32_slice(&[
    const_size_of_u32::<section_up::Immediates>(),
    const_size_of_u32::<section_global::Immediates>(),
    const_size_of_u32::<count::Immediates>(),
    const_size_of_u32::<partial_sum::Immediates>(),
]);

pub const fn const_usize_to_u32(value: usize) -> u32 {
    if size_of::<u32>() >= size_of::<usize>() {
        return value as u32;
    }
    if value > u32::MAX as usize {
        panic!();
    }
    value as u32
}
pub const fn const_usize_to_u64(value: usize) -> u64 {
    if size_of::<u64>() >= size_of::<usize>() {
        return value as u64;
    }
    if value > u64::MAX as usize {
        panic!();
    }
    value as u64
}
pub const fn const_u32_to_usize(value: u32) -> usize {
    if size_of::<usize>() >= size_of::<u32>() {
        return value as usize;
    }
    if value > usize::MAX as u32 {
        panic!();
    }
    value as usize
}
pub const fn const_size_of_u32<T>() -> u32 {
    const_usize_to_u32(size_of::<T>())
}
pub const fn const_max_u32_slice(s: &[u32]) -> u32 {
    let mut max = 0;
    let mut i = 0;

    while i < s.len() {
        if s[i] > max {
            max = s[i];
        }

        i += 1;
    }

    max
}

#[cfg(test)]
mod test {
    use std::sync::OnceLock;

    use crate::IMMEDIATES_SIZE;

    pub struct State {
        pub device: wgpu::Device,
        pub queue: wgpu::Queue,
    }
    static STATE: OnceLock<State> = OnceLock::new();
    pub fn get_state() -> &'static State {
        STATE.get_or_init(|| {
            env_logger::init();

            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

            let adapter = pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
            )
            .unwrap();
            let downlevel_capabilities = adapter.get_downlevel_capabilities();
            if !downlevel_capabilities
                .flags
                .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
            {
                panic!("Adapter does not support compute shaders");
            }

            let mut required_limits = wgpu::Limits::defaults();
            required_limits.max_immediate_size = IMMEDIATES_SIZE;

            let (device, queue) =
                pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::IMMEDIATES | wgpu::Features::SUBGROUP,
                    required_limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                }))
                .expect("Failed to create device");

            State { device, queue }
        })
    }
}
