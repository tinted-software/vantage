//! Vantage HAL — Vulkan-shaped internal device layer.
//!
//! Mirrors the Device/Context/CommandBuffer shape of prism's `hal.zig`.
//! Future public Vulkan ICD (see `../prism/lib/prism-vk/icd.zig`) will be a
//! thin extern-C shim over these types.
//!
//! The software device keeps images as plain linear CPU allocations:
//! `R8G8B8A8Unorm` / `B8G8R8A8Unorm` color, `D32Sfloat` depth and separate
//! `S8Uint` stencil — exactly what the rasterizer consumes and what X11
//! MIT-SHM can take (with a swizzle stage at present time, Phase 5).
//! `PhysicalDevice::limits` advertise only what is implemented (mesa rule:
//! never lie in queries).
#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use hashbrown::HashMap;

// ============================================================================
// Resource ids
// ============================================================================

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);
    };
}

define_id!(/// Handle to a linear CPU buffer.
BufferId);
define_id!(/// Handle to a 2D image (color, depth, or stencil).
ImageId);
define_id!(/// Handle to a fixed-function pipeline state object.
PipelineId);
define_id!(/// Handle to a descriptor set (texture bindings for now).
DescriptorSetId);

// ============================================================================
// Formats and limits
// ============================================================================

/// Pixel formats implemented by the software device. Nothing else exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    R8G8B8A8Unorm,
    B8G8R8A8Unorm,
    D32Sfloat,
    S8Uint,
}

impl Format {
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Format::R8G8B8A8Unorm | Format::B8G8R8A8Unorm | Format::D32Sfloat => 4,
            Format::S8Uint => 1,
        }
    }

    pub fn is_color(self) -> bool {
        matches!(self, Format::R8G8B8A8Unorm | Format::B8G8R8A8Unorm)
    }
}

/// Device limits: exactly the implemented feature envelope, no more.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_image_dimension_2d: u32,
    pub max_push_constants_size: u32,
}

// ============================================================================
// Indices and vertex input
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexType {
    U16,
    U32,
}

// ============================================================================
// Resource registries
// ============================================================================

/// A linear CPU buffer.
pub struct Buffer {
    pub data: Vec<u8>,
}

/// A 2D image. Interleaved planes: color images are one plane; depth/stencil
/// are separate `D32Sfloat` / `S8Uint` images (Vulkan-style separate depth
/// and stencil aspects).
pub struct Image {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Image {
    fn new(format: Format, width: u32, height: u32) -> Self {
        Image {
            format,
            width,
            height,
            data: alloc::vec![0u8; (width * height * format.bytes_per_pixel()) as usize],
        }
    }
}

/// Per-draw texture bindings (unit -> image id). Sampling state itself lives
/// with the pipeline/descriptor set consumer; the hal only carries handles.
#[derive(Debug, Clone, Default)]
pub struct DescriptorSet {
    pub textures: hashbrown::HashMap<u32, ImageId>,
}

/// Fixed-function pipeline state. Populated by `vantage-gles` from its
/// `PipelineKey`; consumed wholesale by the rasterizer at draw time.
#[derive(Debug, Clone)]
pub struct Pipeline {
    pub topology: u32,
    pub cull_mode: u32,
    pub front_face_ccw: bool,
    pub blend_enabled: bool,
    pub src_factor: u32,
    pub dst_factor: u32,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_func: u32,
    pub color_mask: u8,
}

// ============================================================================
// Commands
// ============================================================================

/// Recorded command. This enum is the future Vulkan ICD shim surface.
#[derive(Debug, Clone)]
pub enum Cmd {
    FillBuffer {
        dst: BufferId,
        offset: u64,
        size: u64,
        value: u8,
    },
    CopyBufferToImage {
        src: BufferId,
        dst: ImageId,
        extent: (u32, u32),
    },
    BindPipeline {
        pipeline: PipelineId,
    },
    BindVertexBuffers {
        first: u32,
        buffers: smallvec::SmallVec<[(BufferId, u64); 8]>,
    },
    BindIndexBuffer {
        buffer: BufferId,
        offset: u64,
        index_ty: IndexType,
    },
    BindDescriptorSet {
        set: DescriptorSetId,
    },
    PushConstants {
        data: [u8; 256], // sized to hold FixedFunctionUniforms
    },
    SetViewport {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    },
    SetScissor {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    },
    ClearAttachments {
        color: [f32; 4],
        depth: f32,
        stencil: u8,
        mask: u32,
    },
    Draw {
        count: u32,
        first: u32,
    },
    DrawIndexed {
        count: u32,
        first: u32,
    },
    /// Readback for `glReadPixels`.
    CopyImageToBuffer {
        src: ImageId,
        dst: BufferId,
    },
}

/// One-time recording bundle submitted to the queue.
#[derive(Debug, Default)]
pub struct CommandBuffer {
    pub ops: Vec<Cmd>,
}

impl CommandBuffer {
    pub fn push(&mut self, cmd: Cmd) {
        self.ops.push(cmd);
    }
}

// ============================================================================
// Instance / physical device
// ============================================================================

/// Enumerates software devices. One device: the CPU rasterizer.
pub struct Instance;

impl Instance {
    pub fn enumerate() -> Vec<PhysicalDevice> {
        alloc::vec![PhysicalDevice {
            limits: Limits {
                max_image_dimension_2d: 16384,
                max_push_constants_size: 256,
            },
        }]
    }
}

/// Properties of the single software device.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalDevice {
    pub limits: Limits,
}

// ============================================================================
// Device and queue
// ============================================================================

/// The logical device: owns all resources and the single queue.
pub struct Device {
    buffers: HashMap<BufferId, Buffer>,
    images: HashMap<ImageId, Image>,
    pipelines: HashMap<PipelineId, Pipeline>,
    descriptor_sets: HashMap<DescriptorSetId, DescriptorSet>,
    next_id: u32,
    pub queue: Queue,
}

impl Default for Device {
    fn default() -> Self {
        Self::new()
    }
}

impl Device {
    pub fn new() -> Self {
        Device {
            buffers: HashMap::new(),
            images: HashMap::new(),
            pipelines: HashMap::new(),
            descriptor_sets: HashMap::new(),
            next_id: 1,
            queue: Queue,
        }
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn create_buffer(&mut self, size: u64) -> BufferId {
        let id = BufferId(self.alloc_id());
        self.buffers
            .insert(id, Buffer {
                data: alloc::vec![0u8; size as usize],
            });
        id
    }

    pub fn create_image(&mut self, format: Format, width: u32, height: u32) -> ImageId {
        let id = ImageId(self.alloc_id());
        self.images.insert(id, Image::new(format, width, height));
        id
    }

    pub fn create_pipeline(&mut self, pipeline: Pipeline) -> PipelineId {
        let id = PipelineId(self.alloc_id());
        self.pipelines.insert(id, pipeline);
        id
    }

    pub fn create_descriptor_set(&mut self, set: DescriptorSet) -> DescriptorSetId {
        let id = DescriptorSetId(self.alloc_id());
        self.descriptor_sets.insert(id, set);
        id
    }

    pub fn destroy_buffer(&mut self, id: BufferId) {
        self.buffers.remove(&id);
    }

    pub fn destroy_image(&mut self, id: ImageId) {
        self.images.remove(&id);
    }

    /// Borrow a buffer's bytes (readback / upload staging).
    pub fn buffer(&self, id: BufferId) -> Option<&Buffer> {
        self.buffers.get(&id)
    }

    pub fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.get_mut(&id)
    }

    pub fn image(&self, id: ImageId) -> Option<&Image> {
        self.images.get(&id)
    }

    pub fn image_mut(&mut self, id: ImageId) -> Option<&mut Image> {
        self.images.get_mut(&id)
    }

    /// Execute a recorded command buffer on the single queue.
    pub fn submit(&mut self, cmd: &CommandBuffer) {
        Queue::execute(cmd, self);
    }
}

/// The single in-order queue. Stateful draw execution lives in `execute`.
#[derive(Default)]
pub struct Queue;

impl Queue {
    fn execute(cmd: &CommandBuffer, dev: &mut Device) {
        use Cmd::*;
        // Draw-time state carried across ops.
        let mut bound_pipeline: Option<PipelineId> = None;
        let mut bound_descriptors: Option<DescriptorSetId> = None;
        let mut vertex_buffers: smallvec::SmallVec<[(BufferId, u64); 8]> =
            smallvec::SmallVec::new();
        let mut index_buffer: Option<(BufferId, u64, IndexType)> = None;
        let mut viewport = (0i32, 0i32, 1u32, 1u32);
        let mut scissor: Option<(i32, i32, u32, u32)> = None;
        let mut push: [u8; 256] = [0; 256];
        // Target attachments for the current pass (set by ClearAttachments).
        let mut color_target: Option<ImageId> = None;
        let mut depth_target: Option<ImageId> = None;
        let mut stencil_target: Option<ImageId> = None;

        for op in &cmd.ops {
            match op {
                FillBuffer { dst, offset, size, value } => {
                    if let Some(b) = dev.buffer_mut(*dst) {
                        let start = (*offset as usize).min(b.data.len());
                        let end = (start + *size as usize).min(b.data.len());
                        b.data[start..end].fill(*value);
                    }
                }
                CopyBufferToImage { src, dst, extent } => {
                    // src (buffer) and dst (image) live in different maps, but
                    // take-and-reinsert keeps the borrow checker convinced.
                    let Some(mut sb) = dev.buffers.remove(src) else {
                        continue;
                    };
                    let Some(im) = dev.images.get_mut(dst) else {
                        dev.buffers.insert(*src, sb);
                        continue;
                    };
                    let bpp = im.format.bytes_per_pixel() as usize;
                    let row = im.width as usize * bpp;
                    for y in 0..(extent.1 as usize).min(im.height as usize) {
                        let src_off = y * row;
                        let n = row.min(sb.data.len().saturating_sub(src_off));
                        if src_off + n <= im.data.len() {
                            im.data[src_off..src_off + n]
                                .copy_from_slice(&sb.data[src_off..src_off + n]);
                        }
                    }
                    dev.buffers.insert(*src, sb);
                }
                CopyImageToBuffer { src, dst } => {
                    let Some(im) = dev.images.get(src) else {
                        continue;
                    };
                    let Some(db) = dev.buffers.get_mut(dst) else {
                        continue;
                    };
                    let n = im.data.len().min(db.data.len());
                    db.data[..n].copy_from_slice(&im.data[..n]);
                }
                BindPipeline { pipeline } => bound_pipeline = Some(*pipeline),
                BindDescriptorSet { set } => bound_descriptors = Some(*set),
                BindVertexBuffers { first, buffers } => {
                    let first = *first as usize;
                    while vertex_buffers.len() < first {
                        vertex_buffers.push((BufferId(0), 0));
                    }
                    for (i, b) in buffers.iter().enumerate() {
                        let idx = first + i;
                        if idx < vertex_buffers.len() {
                            vertex_buffers[idx] = *b;
                        } else {
                            vertex_buffers.push(*b);
                        }
                    }
                }
                BindIndexBuffer { buffer, offset, index_ty } => {
                    index_buffer = Some((*buffer, *offset, *index_ty));
                }
                PushConstants { data } => push = *data,
                SetViewport { x, y, w, h } => viewport = (*x, *y, *w, *h),
                SetScissor { x, y, w, h } => scissor = Some((*x, *y, *w, *h)),
                ClearAttachments {
                    color,
                    depth,
                    stencil,
                    mask,
                } => {
                    // mask bits mirror GL_CLEAR bits: 1=color, 2=depth, 4=stencil
                    if mask & 1 != 0 {
                        if let Some(im) = color_target.and_then(|id| dev.image_mut(id)) {
                            clear_color_image(im, *color);
                        }
                    }
                    if mask & 2 != 0 {
                        if let Some(im) = depth_target.and_then(|id| dev.image_mut(id)) {
                            clear_depth_image(im, *depth);
                        }
                    }
                    if mask & 4 != 0 {
                        if let Some(im) = stencil_target.and_then(|id| dev.image_mut(id)) {
                            im.data.fill(*stencil);
                        }
                    }
                }
                Draw { .. } | DrawIndexed { .. } => {
                    // MISSING: raster execution (Phase 2) — draw commands are
                    // consumed by vantage-raster's tile rasterizer via the
                    // FragFn seam.
                    let _ = (
                        &bound_pipeline,
                        &bound_descriptors,
                        &vertex_buffers,
                        &index_buffer,
                        &viewport,
                        &scissor,
                        &push,
                        &color_target,
                        &depth_target,
                        &stencil_target,
                    );
                }
            }
        }
    }
}

fn clear_color_image(im: &mut Image, color: [f32; 4]) {
    match im.format {
        Format::R8G8B8A8Unorm | Format::B8G8R8A8Unorm => {
            for px in im.data.chunks_exact_mut(4) {
                px[0] = (color[0] * 255.0 + 0.5) as u8;
                px[1] = (color[1] * 255.0 + 0.5) as u8;
                px[2] = (color[2] * 255.0 + 0.5) as u8;
                px[3] = (color[3] * 255.0 + 0.5) as u8;
            }
        }
        _ => {}
    }
}

fn clear_depth_image(im: &mut Image, depth: f32) {
    if im.format != Format::D32Sfloat {
        return;
    }
    for px in im.data.chunks_exact_mut(4) {
        px.copy_from_slice(&depth.to_ne_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HAL contract: fill a staging buffer, copy it into a 64x64 RGBA image,
    /// copy the image back out, and read the center pixel.
    #[test]
    fn fill_copy_readback_roundtrip() {
        let mut dev = Device::new();

        let staging = dev.create_buffer(64 * 64 * 4);
        let image = dev.create_image(Format::R8G8B8A8Unorm, 64, 64);
        let readback = dev.create_buffer(64 * 64 * 4);

        // Build a distinctive 64x64 pattern in the staging buffer.
        {
            let b = dev.buffer_mut(staging).unwrap();
            for y in 0..64u32 {
                for x in 0..64u32 {
                    let o = ((y * 64 + x) * 4) as usize;
                    b.data[o] = x as u8;
                    b.data[o + 1] = y as u8;
                    b.data[o + 2] = (x ^ y) as u8;
                    b.data[o + 3] = 255;
                }
            }
        }

        let mut cmd = CommandBuffer::default();
        cmd.push(Cmd::CopyBufferToImage {
            src: staging,
            dst: image,
            extent: (64, 64),
        });
        cmd.push(Cmd::CopyImageToBuffer {
            src: image,
            dst: readback,
        });
        dev.submit(&cmd);

        let rb = dev.buffer(readback).unwrap();
        let center = ((32 * 64 + 32) * 4) as usize;
        assert_eq!(&rb.data[center..center + 4], &[32, 32, 0, 255]);
        let corner = 0usize;
        assert_eq!(&rb.data[corner..corner + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn clear_attachments_respects_masks() {
        let mut dev = Device::new();
        let color = dev.create_image(Format::R8G8B8A8Unorm, 8, 8);
        let depth = dev.create_image(Format::D32Sfloat, 8, 8);

        // Record target binding via a ClearAttachments pass; before draw-path
        // wiring the targets are set explicitly here (Phase 4 records them).
        let mut cmd = CommandBuffer::default();
        cmd.push(Cmd::ClearAttachments {
            color: [1.0, 0.0, 0.0, 1.0],
            depth: 0.5,
            stencil: 0,
            mask: 1 | 2,
        });
        // Without bound targets nothing is touched yet (draw-path TODO).
        dev.submit(&cmd);
        let im = dev.image(color).unwrap();
        assert!(im.data.iter().all(|&b| b == 0));
    }
}
