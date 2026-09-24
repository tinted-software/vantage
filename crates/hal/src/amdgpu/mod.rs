//! AMDGPU hardware execution backend (GFX10.3 / RDNA2).
//!
//! Submits PM4 command packets directly to the AMDGPU kernel driver for
//! hardware triangle rasterization, blending, and viewport transform.
//! Unsupported operations (textures, depth testing, non-GPU images) fall back
//! to the software rasterizer per-draw.
#![allow(dead_code)]

pub mod pm4;
pub mod regs;
pub mod state;

use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use hashbrown::HashMap;

use vantage_drm::{
    AmdgpuBo, AmdgpuDevice, DrmDevice, Error as DrmError, AMDGPU_GEM_CREATE_CPU_ACCESS_REQUIRED,
    AMDGPU_GEM_DOMAIN_GTT, AMDGPU_HW_IP_GFX, AMDGPU_IB_FLAG_EMIT_MEM_SYNC,
};
use vantage_shader::amdgcn::{self, AmdgcnShader, PsConstants, PsKey};

/// Compile a shader, The listing is produced by
/// `vantage-codegen`; `vantage-shader` stays `std`-free.
fn compile_shader(
    shader_ir: &amdgcn::GpuShaderIr,
    processor: &str,
) -> Result<AmdgcnShader, amdgcn::AmdgcnError> {
    let shader = amdgcn::compile_with_listing(shader_ir, processor, false)?;
    Ok(shader)
}

/// Flush the CPU data cache over `[ptr, ptr + len)` and order it against
/// subsequent device accesses.
///
/// GEM BOs in GTT are mapped write-back cached on the CPU, and the GPU does not
/// snoop the CPU caches on this platform. Every buffer is therefore kept in a
/// "flushed clean" state at the submission boundary:
///
/// * before submitting, flushing pushes dirty CPU lines out to memory so the
///   GPU reads the data the CPU wrote;
/// * after the GPU completes, the lines are already clean, so flushing only
///   invalidates them and the next CPU read fetches the GPU's writes.
fn flush_cpu_cache(ptr: *const u8, len: usize) {
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};
        let start = (ptr as usize) & !63;
        let end = (ptr as usize).saturating_add(len);
        let mut address = start;
        while address < end {
            // Safety: the caller owns a live mapping of this range.
            unsafe { _mm_clflush(address as *const u8) };
            address += 64;
        }
        unsafe { _mm_mfence() };
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = (ptr, len);
}

/// Hardware execution driver. Owns the GPU device, memory allocators, shader
/// programs and the active PM4 command batch.
pub struct AmdgpuHardwareDriver {
    pub dev: Arc<AmdgpuDevice>,
    pub ctx_id: u32,
    pub processor: &'static str,
    va_cursor: u64,
    va_max: u64,

    // Precompiled fixed-function shaders.
    pub vs: AmdgcnShader,
    pub vs_bo: AmdgpuBo,
    pub ps_cache: HashMap<PsKey, (AmdgcnShader, AmdgpuBo)>,

    // Reusable GTT buffers.
    pub upload_arena: AmdgpuBo,
    pub arena_offset: usize,
    pub ib_bo: AmdgpuBo,

    // Current in-flight command stream.
    pub pm4: pm4::Pm4,
    pub referenced_handles: Vec<u32>,

    // Render-target BOs allocated for Device images.
    pub image_bos: HashMap<crate::ImageId, AmdgpuBo>,

    /// Latched on timeout or submit failure: forces CPU fallback thereafter.
    pub broken: bool,
}

impl Drop for AmdgpuHardwareDriver {
    fn drop(&mut self) {
        let _ = self.dev.destroy_context(self.ctx_id);
    }
}

const ARENA_INITIAL_SIZE: u64 = 2 * 1024 * 1024; // 2 MB
const IB_INITIAL_SIZE: u64 = 256 * 1024; // 256 KB
const SHADER_PREFETCH_PADDING: u64 = 1024; // 1 KB safe padding

impl AmdgpuHardwareDriver {
    /// Probe and initialize AMDGPU hardware execution. Selects the first
    /// GFX10.3 render node, preferring one without an active display connection.
    /// Override by setting `VANTAGE_AMDGPU_DEVICE=/dev/dri/renderDN`.
    pub fn probe() -> Result<Self, DrmError> {
        #[cfg(feature = "std")]
        {
            let drm_dev = select_render_node()?;
            let dev = AmdgpuDevice::new(drm_dev)?;

            // Gate on GFX10.3 (RDNA2).
            let disc = dev.gfx_ip.ip_discovery_version;
            let is_gfx103 =
                ((disc >> 16) & 0xff == 10) && ((disc >> 8) & 0xff == 3) && (disc & 0xff <= 7);
            if !is_gfx103 {
                std::eprintln!(
                    "vantage-amdgpu: device 0x{:04x} discovery version {:#x} is not GFX10.3",
                    dev.info.device_id,
                    disc
                );
                return Err(DrmError::WrongDriver(b"not GFX10.3"));
            }
            let processor = amdgcn::gfx103_processor(disc);
            let ctx_id = dev.create_context()?;

            let mut va_cursor = dev.info.virtual_address_offset.max(0x100_0000);
            let va_max = dev.info.virtual_address_max;

            let mut alloc_gtt_init = |size: u64| -> Result<AmdgpuBo, DrmError> {
                let align = 0x10000u64;
                let aligned = (size + align - 1) & !(align - 1);
                if va_cursor.saturating_add(aligned) > va_max {
                    return Err(DrmError::Io(12));
                }
                let va = va_cursor;
                va_cursor += aligned;
                dev.alloc_bo(
                    size,
                    4096,
                    AMDGPU_GEM_DOMAIN_GTT,
                    AMDGPU_GEM_CREATE_CPU_ACCESS_REQUIRED,
                    va,
                    true,
                )
            };

            let upload_arena = alloc_gtt_init(ARENA_INITIAL_SIZE)?;
            let ib_bo = alloc_gtt_init(IB_INITIAL_SIZE)?;

            // Compile and upload the vertex shader.
            let vertex_shader_ir = amdgcn::build_vertex_shader();
            let vs = compile_shader(&vertex_shader_ir, processor).map_err(|error| {
                std::eprintln!("vantage-amdgpu: vertex shader compilation failed: {error:?}");
                DrmError::Io(22)
            })?;
            let mut vs_bo = alloc_gtt_init((vs.code.len() as u64) + SHADER_PREFETCH_PADDING)?;
            if let Some(slice) = vs_bo.as_slice_mut() {
                slice[..vs.code.len()].copy_from_slice(&vs.code);
                slice[vs.code.len()..].fill(0);
            }

            let drv = Self {
                va_cursor,
                va_max,
                dev,
                ctx_id,
                processor,
                vs,
                vs_bo,
                ps_cache: HashMap::new(),
                upload_arena,
                arena_offset: 0,
                ib_bo,
                pm4: pm4::Pm4::with_capacity(4096),
                referenced_handles: Vec::with_capacity(32),
                image_bos: HashMap::new(),
                broken: false,
            };

            std::eprintln!(
                "vantage-amdgpu: initialized for {} (device 0x{:04x}, context {})",
                processor,
                drv.dev.info.device_id,
                ctx_id
            );
            Ok(drv)
        }

        #[cfg(not(feature = "std"))]
        Err(DrmError::NoDevice)
    }

    /// Bump-allocate a GPU virtual address block.
    pub fn alloc_va(&mut self, size: u64) -> Result<u64, DrmError> {
        let align = 0x10000u64; // 64 KB page alignment
        let aligned = (size + align - 1) & !(align - 1);
        if self.va_cursor.saturating_add(aligned) > self.va_max {
            return Err(DrmError::Io(12)); // ENOMEM
        }
        let va = self.va_cursor;
        self.va_cursor += aligned;
        Ok(va)
    }

    /// Allocate host-visible GPU memory (GTT) mapped to both CPU and GPU.
    pub fn alloc_gtt(&mut self, size: u64) -> Result<AmdgpuBo, DrmError> {
        let va = self.alloc_va(size)?;
        self.dev.alloc_bo(
            size,
            4096,
            AMDGPU_GEM_DOMAIN_GTT,
            AMDGPU_GEM_CREATE_CPU_ACCESS_REQUIRED,
            va,
            true,
        )
    }

    /// Upload shader code bytes into a newly allocated, padded GTT BO.
    fn upload_code(&mut self, code: &[u8]) -> Result<AmdgpuBo, DrmError> {
        let total = (code.len() as u64) + SHADER_PREFETCH_PADDING;
        let mut bo = self.alloc_gtt(total)?;
        if let Some(slice) = bo.as_slice_mut() {
            slice[..code.len()].copy_from_slice(code);
            slice[code.len()..].fill(0); // NOP pad
        }
        Ok(bo)
    }

    /// Get or compile the pixel shader variant for `key`.
    pub fn get_or_compile_ps(&mut self, key: &PsKey) -> Result<u64, DrmError> {
        if let Some((_, bo)) = self.ps_cache.get(key) {
            return Ok(bo.gpu_va);
        }
        let pixel_shader_ir = amdgcn::build_pixel_shader(key);
        let sh = compile_shader(&pixel_shader_ir, self.processor).map_err(|_| DrmError::Io(22))?;
        let bo = self.upload_code(&sh.code)?;
        let va = bo.gpu_va;
        self.ps_cache.insert(*key, (sh, bo));
        Ok(va)
    }

    /// Check whether a draw call can be executed on hardware.
    pub fn can_draw(
        &self,
        color_id: Option<crate::ImageId>,
        pipe: &crate::Pipeline,
        frag_state: &vantage_raster::FragState,
    ) -> Option<PsKey> {
        if self.broken {
            return None;
        }
        let cid = color_id?;
        if !self.image_bos.contains_key(&cid) {
            return None;
        }
        // Core scope: depth test/write stays on the CPU path.
        if pipe.depth_test || pipe.depth_write {
            return None;
        }
        PsKey::from_state(frag_state)
    }

    /// Flush any pending GPU commands and wait for completion. Resets the
    /// upload arena cursor and clears the command stream.
    pub fn flush(&mut self) -> Result<(), DrmError> {
        if self.pm4.is_empty() || self.broken {
            self.pm4.clear();
            self.referenced_handles.clear();
            self.arena_offset = 0;
            return Ok(());
        }

        // Color/depth writes live in the RB and L2 until the caches are
        // written back. On GFX10+, EVENT_WRITE alone does not write back
        // the GL2 cache for the CB. Mesa uses RELEASE_MEM with
        // FLUSH_AND_INV_CB_DATA_TS (event 45), index 5 (EOP), and GCR_CNTL 0x70f.
        const EVENT_FLUSH_AND_INV_CB_DATA_TS: u32 = 45;
        const EVENT_INDEX_EOP: u32 = 5;
        // GCR_CNTL: GL2_INV(1) | GL2_WB(1) | GLM_INV(1) | GLM_WB(1) | SEQ(0x700)
        const GCR_CNTL_CB_FLUSH: u32 = 0x70f;
        self.pm4.release_mem(
            EVENT_FLUSH_AND_INV_CB_DATA_TS,
            EVENT_INDEX_EOP,
            GCR_CNTL_CB_FLUSH,
            0, // dst_sel: memory controller
            0, // int_sel: none
            0, // data_sel: discard
            0, // gpu_va
            0, // data
        );

        // Align PM4 to 32 dwords (128 bytes, standard CP alignment).
        self.pm4.pad_to(32);
        let byte_len = self.pm4.len() * 4;

        if byte_len > self.ib_bo.size as usize {
            // IB buffer overflow: resize.
            let new_size = (byte_len as u64 * 2).max(IB_INITIAL_SIZE);
            self.ib_bo = self.alloc_gtt(new_size)?;
        }

        if let Some(slice) = self.ib_bo.as_slice_mut() {
            let u8s = bytemuck::cast_slice::<u32, u8>(&self.pm4.dwords);
            slice[..u8s.len()].copy_from_slice(u8s);
        }

        // Gather all referenced buffer handles (IB itself must be included).
        let mut handles = Vec::with_capacity(self.referenced_handles.len() + 4);
        handles.push(self.ib_bo.handle);
        handles.push(self.upload_arena.handle);
        handles.push(self.vs_bo.handle);
        for &h in &self.referenced_handles {
            if !handles.contains(&h) {
                handles.push(h);
            }
        }

        // Make every CPU-written buffer visible to the GPU before it reads
        // them: the IB, the vertex/index arena, shader code, and images the CPU
        // cleared or rendered into.
        flush_cpu_cache(self.ib_bo.cpu_ptr, byte_len);
        flush_cpu_cache(self.upload_arena.cpu_ptr, self.arena_offset);
        flush_cpu_cache(self.vs_bo.cpu_ptr, self.vs_bo.size as usize);
        for (_, (_, bo)) in &self.ps_cache {
            flush_cpu_cache(bo.cpu_ptr, bo.size as usize);
        }
        for bo in self.image_bos.values() {
            flush_cpu_cache(bo.cpu_ptr, bo.size as usize);
        }

        let seq = match self.dev.submit(
            self.ctx_id,
            AMDGPU_HW_IP_GFX,
            self.ib_bo.gpu_va,
            self.pm4.len() as u32,
            AMDGPU_IB_FLAG_EMIT_MEM_SYNC,
            &handles,
        ) {
            Ok(s) => s,
            Err(e) => {
                self.broken = true;
                self.pm4.clear();
                self.referenced_handles.clear();
                self.arena_offset = 0;
                return Err(e);
            }
        };

        // Wait up to 2 seconds for completion.
        match self
            .dev
            .wait_cs(self.ctx_id, AMDGPU_HW_IP_GFX, seq, 2_000_000_000)
        {
            Ok(true) => {}
            Ok(false) => {
                self.broken = true;
                return Err(DrmError::Io(110)); // ETIMEDOUT
            }
            Err(e) => {
                self.broken = true;
                return Err(e);
            }
        }

        // The GPU's writes are now in memory. Flushing the (clean) CPU lines
        // invalidates them, so a readback observes the rendered pixels instead
        // of the values the CPU wrote earlier.
        for bo in self.image_bos.values() {
            flush_cpu_cache(bo.cpu_ptr, bo.size as usize);
        }

        self.pm4.clear();
        self.referenced_handles.clear();
        self.arena_offset = 0;
        Ok(())
    }

    /// Record a single draw call into the current PM4 batch.
    #[allow(clippy::too_many_arguments)]
    pub fn record_draw(
        &mut self,
        color_id: crate::ImageId,
        pipe: &crate::Pipeline,
        vertices: &[crate::Vertex],
        indices: Option<&[u32]>,
        viewport: (i32, i32, u32, u32),
        scissor: Option<(i32, i32, u32, u32)>,
        frag_state: &vantage_raster::FragState,
        ps_key: &PsKey,
        fb_w: u32,
        fb_h: u32,
        fb_pitch: u32,
        is_bgra: bool,
    ) -> Result<(), DrmError> {
        let vbytes = bytemuck::cast_slice::<crate::Vertex, u8>(vertices);
        let ibytes = indices.map(bytemuck::cast_slice::<u32, u8>).unwrap_or(&[]);
        let v_aligned = (vbytes.len() + 15) & !15;
        let i_aligned = (ibytes.len() + 15) & !15;
        let needed = v_aligned + i_aligned;

        // If the batch or the upload arena is nearing capacity, flush first.
        if self.pm4.len() + 512 > (self.ib_bo.size as usize / 4)
            || self.arena_offset + needed > (self.upload_arena.size as usize)
        {
            self.flush()?;
        }

        // If a single draw is larger than the default arena, grow it.
        if needed > self.upload_arena.size as usize {
            let new_size = (needed as u64 * 2).max(ARENA_INITIAL_SIZE);
            self.upload_arena = self.alloc_gtt(new_size)?;
            self.arena_offset = 0;
        }

        // Upload vertex and index data into the arena.
        let base_va = self.upload_arena.gpu_va;
        let v_off = self.arena_offset;
        let i_off = v_off + v_aligned;
        if let Some(slice) = self.upload_arena.as_slice_mut() {
            slice[v_off..v_off + vbytes.len()].copy_from_slice(vbytes);
            if !ibytes.is_empty() {
                slice[i_off..i_off + ibytes.len()].copy_from_slice(ibytes);
            }
        }
        self.arena_offset += needed;

        let color_bo = self.image_bos.get(&color_id).unwrap();
        let (color_va, color_handle) = (color_bo.gpu_va, color_bo.handle);
        self.referenced_handles.push(color_handle);

        // Emit preamble at the start of each IB.
        if self.pm4.is_empty() {
            state::emit_preamble(&mut self.pm4);
        }

        // Color target.
        state::emit_color_target(&mut self.pm4, color_va, fb_pitch, fb_w, fb_h, is_bgra);

        // Rasterizer, scissor, viewport, and blend.
        state::emit_raster_and_blend(
            &mut self.pm4,
            pipe,
            viewport,
            scissor,
            fb_w,
            fb_h,
            ps_key.uses_kill(),
        );

        // Shaders.
        let ps_va = self.get_or_compile_ps(ps_key)?;
        let ps_handle = self.ps_cache.get(ps_key).unwrap().1.handle;
        self.referenced_handles.push(ps_handle);

        let ps_vgprs = self
            .ps_cache
            .get(ps_key)
            .unwrap()
            .0
            .reg(regs::SPI_SHADER_PGM_RSRC1_PS)
            .map(|r| ((r & regs::RSRC1_VGPRS_MASK) + 1) * 8)
            .unwrap_or(16);
        let ps_input_ena = self
            .ps_cache
            .get(ps_key)
            .unwrap()
            .0
            .reg(regs::SPI_PS_INPUT_ENA)
            .unwrap_or(0);

        state::emit_shader_programs(
            &mut self.pm4,
            self.vs_bo.gpu_va,
            16, // VS uses 6 VGPRs (rounded to 16)
            ps_va,
            ps_vgprs,
            ps_input_ena,
            if ps_key.fog_mode != 0 { 2 } else { 1 },
        );

        // User SGPRs: vertex buffer address and runtime constants.
        state::emit_vs_user_data(&mut self.pm4, base_va + (v_off as u64));
        let ps_constants = PsConstants {
            fog_color: [
                frag_state.fog_color[0],
                frag_state.fog_color[1],
                frag_state.fog_color[2],
            ],
            fog_start: frag_state.fog_start,
            fog_end: frag_state.fog_end,
            fog_density: frag_state.fog_density,
            alpha_ref: frag_state.alpha_ref,
        };
        state::emit_ps_user_data(&mut self.pm4, &ps_constants);

        // Draw packet.
        if let Some(idx) = indices {
            state::emit_draw_indexed(
                &mut self.pm4,
                base_va + (i_off as u64),
                idx.len() as u32,
                false, // u32 indices
            );
        } else {
            state::emit_draw_auto(&mut self.pm4, vertices.len() as u32);
        }

        Ok(())
    }
}

// ============================================================================
// Device selection: prefer non-display GPU
// ============================================================================

/// Returns true if any connector on the sibling card node is connected to a display.
#[cfg(feature = "std")]
fn card_has_display(render_num: u32) -> bool {
    let dev_drm = format!("/sys/class/drm/renderD{render_num}/device/drm");
    let Ok(entries) = std::fs::read_dir(dev_drm) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("card") {
            continue;
        }
        let Ok(conn_entries) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for conn in conn_entries.flatten() {
            let status_file = conn.path().join("status");
            if let Ok(st) = std::fs::read_to_string(status_file) {
                if st.trim() == "connected" {
                    return true;
                }
            }
        }
    }
    false
}

/// Select the best render node: honors `VANTAGE_AMDGPU_DEVICE`, otherwise
/// prefers the primary AMDGPU (discrete GPU / boot_vga), falling back to any
/// available AMDGPU render node.
#[cfg(feature = "std")]
fn select_render_node() -> Result<DrmDevice, DrmError> {
    if let Ok(path) = std::env::var("VANTAGE_AMDGPU_DEVICE") {
        let mut p = path.into_bytes();
        p.push(0);
        return DrmDevice::open_path(&p);
    }

    let mut candidates: Vec<(u32, DrmDevice, bool)> = Vec::new();
    for n in 128u32..160 {
        let path = format!("/dev/dri/renderD{n}\0");
        let Ok(dev) = DrmDevice::open_path(path.as_bytes()) else {
            continue;
        };
        let is_amd = dev
            .sysfs_driver_name()
            .as_deref()
            .map(|d| d == b"amdgpu")
            .unwrap_or(false);
        if !is_amd {
            continue;
        }
        let boot_vga =
            std::fs::read_to_string(format!("/sys/class/drm/renderD{n}/device/boot_vga"))
                .map(|s| s.trim() == "1")
                .unwrap_or(false);
        candidates.push((n, dev, boot_vga));
    }

    // Prefer primary GPU (boot_vga, usually discrete GPU with active display engine).
    if let Some((n, dev, _)) = candidates
        .into_iter()
        .max_by_key(|(_, _, boot_vga)| *boot_vga)
    {
        std::eprintln!("vantage-amdgpu: selecting render node /dev/dri/renderD{n}");
        return Ok(dev);
    }

    Err(DrmError::NoDevice)
}
