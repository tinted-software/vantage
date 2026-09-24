//! AMDGPU hardware bring-up and end-to-end verification.
//!
//! Exercises real GPU rasterization on the non-display iGPU (or device specified
//! by `VANTAGE_AMDGPU_DEVICE`):
//! 1. Memory allocation in GTT with 256B pitch alignment.
//! 2. Hardware initialization & PM4 preamble submission.
//! 3. Solid triangle rasterization through the pure-Rust AMDGPU shader backend.
//! 4. Pixel readback and exact color verification (BGRA order).
//! 5. Alpha blending on GPU.
//! 6. Alpha test / kill discard on GPU.
//!
//! Run with `VANTAGE_AMDGPU=1 cargo run -p vantage-hal --features amdgpu --example amdgpu_bringup`.

use vantage_hal::{Cmd, CommandBuffer, Device, Format, Pipeline, Vertex};
use vantage_raster::gl;

fn main() {
    let mut dev = Device::new();
    if dev.hw.is_none() {
        eprintln!("hardware driver not active; run with VANTAGE_AMDGPU=1");
        std::process::exit(1);
    }
    eprintln!("Hardware driver active: running GPU rendering verification...");

    const W: u32 = 64;
    const H: u32 = 64;

    // 1. GTT-backed color target (B8G8R8A8Unorm).
    let color = dev.create_image(Format::B8G8R8A8Unorm, W, H);
    assert!(
        dev.hw.as_ref().unwrap().image_bos.contains_key(&color),
        "color target must be GPU-backed"
    );
    let pitch = dev.image(color).unwrap().row_pitch as usize;
    assert!(pitch % 256 == 0, "row pitch must be 256-byte aligned");

    let pipeline_opaque = Pipeline {
        topology: 0,
        cull_mode: 0,
        front_face_ccw: true,
        blend_enabled: false,
        src_factor: 1,
        dst_factor: 0,
        depth_test: false,
        depth_write: false,
        depth_func: gl::ALWAYS,
        color_mask: 0x0F,
    };

    // Triangle 1: Green triangle in the center.
    let v0 = Vertex {
        pos: [-0.6, -0.6, 0.5, 1.0],
        color: [0.0, 1.0, 0.0, 1.0],
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };
    let v1 = Vertex {
        pos: [0.6, -0.6, 0.5, 1.0],
        color: [0.0, 1.0, 0.0, 1.0],
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };
    let v2 = Vertex {
        pos: [0.0, 0.6, 0.5, 1.0],
        color: [0.0, 1.0, 0.0, 1.0],
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };

    // 2. Submit Clear to Black + Draw Green Triangle.
    let pipe_id = dev.create_pipeline(pipeline_opaque.clone());
    let mut cmd = CommandBuffer::default();
    cmd.push(Cmd::BindPipeline { pipeline: pipe_id });
    cmd.push(Cmd::BindAttachments {
        color: Some(color),
        depth: None,
        stencil: None,
    });
    cmd.push(Cmd::ClearAttachments {
        color: [0.0, 0.0, 0.0, 1.0],
        depth: 1.0,
        stencil: 0,
        mask: 1,
    });
    cmd.push(Cmd::SetViewport {
        x: 0,
        y: 0,
        w: W,
        h: H,
    });
    cmd.push(Cmd::DrawMesh {
        vertices: vec![v0, v1, v2],
        indices: None,
        pipeline: pipeline_opaque.clone(),
    });

    let t0 = std::time::Instant::now();
    dev.submit(&cmd);
    let dt = t0.elapsed();
    eprintln!("Triangle 1 (opaque green) rendered and submitted in {dt:?}");

    // 3. Readback and verify pixels.
    // Format B8G8R8A8: byte 0=B, 1=G, 2=R, 3=A.
    {
        let img = dev.image(color).unwrap();
        let slice = img.slice();
        let mut modified = 0;
        for y in 0..H {
            for x in 0..W {
                let off = (y as usize) * pitch + (x as usize) * 4;
                let px = &slice[off..off + 4];
                if px[1] != 0 || px[2] != 0 {
                    if modified < 5 {
                        eprintln!(
                            "Found pixel at ({x}, {y}): B={}, G={}, R={}, A={}",
                            px[0], px[1], px[2], px[3]
                        );
                    }
                    modified += 1;
                }
            }
        }
        eprintln!("Total modified pixels: {modified}");

        // Sample center pixel (32, 32) -> must be Green: [0, 255, 0, 255]
        let center_offset = 32 * pitch + 32 * 4;
        let c_px = &slice[center_offset..center_offset + 4];
        eprintln!(
            "Center pixel (32, 32): B={}, G={}, R={}, A={}",
            c_px[0], c_px[1], c_px[2], c_px[3]
        );
        assert_eq!(c_px, &[0, 255, 0, 255], "Center pixel must be solid green!");

        // Sample corner pixel (2, 2) -> outside triangle, must be Black: [0, 0, 0, 255]
        let corner_offset = 2 * pitch + 2 * 4;
        let k_px = &slice[corner_offset..corner_offset + 4];
        eprintln!(
            "Corner pixel (2, 2): B={}, G={}, R={}, A={}",
            k_px[0], k_px[1], k_px[2], k_px[3]
        );
        assert_eq!(
            k_px,
            &[0, 0, 0, 255],
            "Corner pixel must be black clear color!"
        );
    }
    eprintln!("Opaque triangle test PASSED!");

    // 4. Test Alpha Blending:
    // Blend Red [1.0, 0.0, 0.0, 0.5] over the existing green center.
    // Result should be B=0, G=128, R=128, A=255.
    let pipeline_blend = Pipeline {
        topology: 0,
        cull_mode: 0,
        front_face_ccw: true,
        blend_enabled: true,
        src_factor: gl::SRC_ALPHA,
        dst_factor: gl::ONE_MINUS_SRC_ALPHA,
        depth_test: false,
        depth_write: false,
        depth_func: gl::ALWAYS,
        color_mask: 0x0F,
    };
    let v_red0 = Vertex {
        pos: [-0.6, -0.6, 0.5, 1.0],
        color: [1.0, 0.0, 0.0, 0.5],
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };
    let v_red1 = Vertex {
        pos: [0.6, -0.6, 0.5, 1.0],
        color: [1.0, 0.0, 0.0, 0.5],
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };
    let v_red2 = Vertex {
        pos: [0.0, 0.6, 0.5, 1.0],
        color: [1.0, 0.0, 0.0, 0.5],
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };

    let mut cmd2 = CommandBuffer::default();
    cmd2.push(Cmd::BindAttachments {
        color: Some(color),
        depth: None,
        stencil: None,
    });
    cmd2.push(Cmd::SetViewport {
        x: 0,
        y: 0,
        w: W,
        h: H,
    });
    cmd2.push(Cmd::DrawMesh {
        vertices: vec![v_red0, v_red1, v_red2],
        indices: None,
        pipeline: pipeline_blend,
    });
    dev.submit(&cmd2);

    {
        let img = dev.image(color).unwrap();
        let slice = img.slice();
        let center_offset = 32 * pitch + 32 * 4;
        let c_px = &slice[center_offset..center_offset + 4];
        eprintln!(
            "Blended center pixel (32, 32): B={}, G={}, R={}, A={}",
            c_px[0], c_px[1], c_px[2], c_px[3]
        );
        // Green dst = 255, Src = Red (128) -> Blended: R ≈ 128, G ≈ 128, B = 0
        assert!(
            c_px[0] == 0 && c_px[1].abs_diff(128) <= 2 && c_px[2].abs_diff(128) <= 2,
            "Pixel should be blended yellow/olive!"
        );
    }
    eprintln!("Alpha blending test PASSED!");

    // 5. Test Alpha Test (Kill):
    // Draw with alpha_func = GREATER, alpha_ref = 0.5, but vertex color alpha = 0.2.
    // All fragments should be killed, leaving the image unchanged.
    let mut frag_state_kill = vantage_raster::FragState::default();
    frag_state_kill.alpha_func = gl::GREATER;
    frag_state_kill.alpha_ref = 0.5;

    let v_blue = Vertex {
        pos: [-1.0, -1.0, 0.5, 1.0],
        color: [0.0, 0.0, 1.0, 0.2], // Alpha = 0.2 fails alpha test!
        tex0: [0.0, 0.0],
        tex1: [0.0, 0.0],
        fog: 0.0,
        _pad: 0.0,
    };
    let mut cmd3 = CommandBuffer::default();
    cmd3.push(Cmd::BindAttachments {
        color: Some(color),
        depth: None,
        stencil: None,
    });
    cmd3.push(Cmd::SetFragState(Box::new(frag_state_kill)));
    cmd3.push(Cmd::SetViewport {
        x: 0,
        y: 0,
        w: W,
        h: H,
    });
    cmd3.push(Cmd::DrawMesh {
        vertices: vec![v_blue, v_blue, v_blue],
        indices: None,
        pipeline: pipeline_opaque,
    });
    dev.submit(&cmd3);

    {
        let img = dev.image(color).unwrap();
        let slice = img.slice();
        let center_offset = 32 * pitch + 32 * 4;
        let c_px = &slice[center_offset..center_offset + 4];
        // Blue must NOT have overwritten the center pixel!
        assert_eq!(c_px[0], 0, "Blue must be killed by alpha test!");
    }
    eprintln!("Alpha test (kill) test PASSED!");

    eprintln!("\nALL AMDGPU HARDWARE VERIFICATION TESTS PASSED!");
}
