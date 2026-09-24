//! Pure-Rust GPU machine-code generation from pliron's LLVM dialect.
//!
//! `pliron-llvm` defines the input dialect; this crate does not link LLVM.
//! A target supplies the hardware ABI, pseudo-instruction expansion, machine
//! encoding, and loader-visible register configuration.

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use pliron::context::{Context, Ptr};
use pliron::operation::Operation;

pub mod instruction_selection;
pub mod machine;
pub mod register_allocation;
pub mod targets;

use instruction_selection::SelectionError;
use register_allocation::RegisterAllocationError;
use targets::{Gfx10Error, Signature, Summary, Target};

#[derive(Debug)]
pub enum Error {
    UnknownProcessor,
    InstructionSelection(SelectionError),
    RegisterAllocation(RegisterAllocationError),
    TargetExpansion(Gfx10Error),
    Encoding(Gfx10Error),
}

/// Machine code and the registers the driver must program for its entry ABI.
pub struct CompiledShader {
    pub code: Vec<u8>,
    pub configuration: Vec<(u32, u32)>,
    /// Human-readable instruction listing, allocated only when requested.
    pub assembly: Option<String>,
}

/// Compile the `main` shader function for an AMDGPU processor without
/// producing a diagnostic instruction listing.
pub fn compile(
    context: &Context,
    module: Ptr<Operation>,
    signature: &Signature,
    processor: &str,
) -> Result<CompiledShader, Error> {
    compile_with_listing(context, module, signature, processor, false)
}

/// Compile the `main` shader function, optionally producing an instruction
/// listing for diagnostics.
pub fn compile_with_listing(
    context: &Context,
    module: Ptr<Operation>,
    signature: &Signature,
    processor: &str,
    include_listing: bool,
) -> Result<CompiledShader, Error> {
    let target = targets::for_processor(processor).ok_or(Error::UnknownProcessor)?;
    compile_for_target(context, module, signature, &target, include_listing)
}

/// Compile against a target implementation; useful when adding another GPU
/// generation without changing the compiler pipeline.
pub fn compile_for_target(
    context: &Context,
    module: Ptr<Operation>,
    signature: &Signature,
    target: &impl Target,
    include_listing: bool,
) -> Result<CompiledShader, Error> {
    let mut function =
        instruction_selection::select_instructions(context, module, signature, target)
            .map_err(Error::InstructionSelection)?;
    target
        .expand_pseudo_instructions(&mut function)
        .map_err(Error::TargetExpansion)?;
    let allocation = register_allocation::allocate_registers(&function, target)
        .map_err(Error::RegisterAllocation)?;
    let summary = Summary::from_function(&function, &allocation);
    let code = target
        .encode_function(&function, &allocation)
        .map_err(Error::Encoding)?;
    let assembly = if include_listing {
        let mut listing = String::new();
        for instruction_index in 0..function.instructions.len() {
            if instruction_index != 0 {
                listing.push('\n');
            }
            listing.push_str(&target.format_instruction(&function, &allocation, instruction_index));
        }
        Some(listing)
    } else {
        None
    };
    Ok(CompiledShader {
        code,
        configuration: target.configuration(signature.stage, &summary),
        assembly,
    })
}
