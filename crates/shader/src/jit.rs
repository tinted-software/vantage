//! Cranelift JIT compilation backend for Vantage Fragment programs (`std` only).

#![allow(dead_code)]

use crate::lowering::lower_to_clif;
use crate::program::FragmentIr;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use vantage_raster::{FragFn, FragState, Varyings};

/// A compiled, natively callable fragment program managed by Cranelift JIT.
pub struct JitFragmentProgram {
    module: JITModule,
    func_id: FuncId,
    code_ptr: *const u8,
}

unsafe impl Send for JitFragmentProgram {}
unsafe impl Sync for JitFragmentProgram {}

impl JitFragmentProgram {
    /// Compile a [`FragmentIr`] function to executable machine code via Cranelift JIT.
    pub fn compile(ir: &FragmentIr) -> Result<Self, alloc::string::String> {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| alloc::format!("failed to set opt_level: {:?}", e))?;
        flag_builder
            .set("is_pic", "true")
            .map_err(|e| alloc::format!("failed to set is_pic: {:?}", e))?;

        let isa_builder = cranelift_native::builder()
            .map_err(|e| alloc::format!("failed to create native ISA builder: {}", e))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| alloc::format!("failed to create ISA: {}", e))?;

        let mut jit_builder = JITBuilder::with_isa(isa, default_libcall_names());

        // Symbol resolution for external helper calls like @vantage_expf
        jit_builder.symbol("vantage_expf", vantage_expf as *const u8);

        let mut module = JITModule::new(jit_builder);

        // Exponential fog calls @vantage_expf: declare it as a module import
        // so lowering can build a module-resolvable FuncRef (the symbol
        // provider above maps the declaration name to the host function).
        let expf_fid = if ir.has_external_calls() {
            let sig = cranelift_codegen::ir::Signature {
                params: alloc::vec![cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::F32
                )],
                returns: alloc::vec![cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::F32
                )],
                call_conv: module.isa().default_call_conv(),
            };
            Some(
                module
                    .declare_function(crate::program::EXPF_NAME, Linkage::Import, &sig)
                    .map_err(|e| alloc::format!("failed to declare expf import: {:?}", e))?,
            )
        } else {
            None
        };

        let clif_fn = lower_to_clif(ir, expf_fid.map(|f| f.as_u32()));

        let func_id = module
            .declare_function("frag_entry", Linkage::Export, &clif_fn.signature)
            .map_err(|e| alloc::format!("failed to declare function: {:?}", e))?;

        let mut ctx = module.make_context();
        ctx.func = clif_fn;

        module
            .define_function(func_id, &mut ctx)
            .map_err(|e| alloc::format!("failed to define function: {:?}", e))?;

        module.clear_context(&mut ctx);
        module
            .finalize_definitions()
            .map_err(|e| alloc::format!("failed to finalize definitions: {:?}", e))?;

        let code_ptr = module.get_finalized_function(func_id);

        Ok(Self {
            module,
            func_id,
            code_ptr,
        })
    }

    /// Return an unsafe function pointer matching [`FragFn`].
    #[inline]
    pub fn as_frag_fn(&self) -> FragFn {
        unsafe { core::mem::transmute(self.code_ptr) }
    }
}

/// External C ABI helper for expf when invoked from JIT code.
#[no_mangle]
pub extern "C" fn vantage_expf(x: f32) -> f32 {
    libm::expf(x)
}
