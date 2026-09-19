//! Lowers pliron fragment IR to Cranelift IR (`cranelift_codegen::ir::Function`).
//!
//! Traverses the `llvm.func @frag_entry` operation built by `program.rs`,
//! mapping each pliron basic block, value, and operation to its Cranelift equivalent.

#![allow(dead_code)]

use crate::program::FragmentIr;
use alloc::vec::Vec;
use cranelift_codegen::ir::{
    types, AbiParam, Block, BlockCall, Function, InstBuilder, MemFlags, Signature,
    Value as ClifValue,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use hashbrown::HashMap;
use pliron::attribute::attr_cast;
use pliron::builtin::op_interfaces::BranchOpInterface;
use pliron::builtin::op_interfaces::OneResultInterface;
use pliron::builtin::type_interfaces::FunctionTypeInterface;
use pliron::builtin::types::{FP32Type, IntegerType};
use pliron::context::Ptr;
use pliron::operation::Operation;
use pliron::printable::Printable;
use pliron::r#type::{Type, Typed};
use pliron::value::Value as PlironValue;
use pliron_llvm::attributes::{FCmpPredicateAttr, ICmpPredicateAttr};
use pliron_llvm::op_interfaces::CastOpInterface;
use pliron_llvm::ops::*;
use pliron_llvm::types::{FuncType, PointerType};

/// Map a pliron type to a Cranelift scalar type.
fn lower_type(ctx: &pliron::context::Context, ty: pliron::r#type::TypeHandle) -> types::Type {
    let t = ty.deref(ctx);
    if t.is::<PointerType>() {
        types::I64
    } else if let Some(it) = t.downcast_ref::<IntegerType>() {
        match it.width() {
            1 => types::I8, // represent i1 flags as i8 for simplicity in Cranelift
            8 => types::I8,
            16 => types::I16,
            32 => types::I32,
            64 => types::I64,
            w => panic!("unsupported integer width {w}"),
        }
    } else if t.is::<FP32Type>() {
        types::F32
    } else {
        panic!("unsupported pliron type for lowering: {:?}", ty);
    }
}

/// Downcast a raw operation pointer into its concrete typed wrapper.
/// Host target frontend config (pointer width + endianness) for finalize.
fn default_frontend_config() -> cranelift_codegen::isa::TargetFrontendConfig {
    let flags = cranelift_codegen::settings::Flags::new(cranelift_codegen::settings::builder());
    let isa = cranelift_native::builder()
        .expect("native target")
        .finish(flags)
        .expect("native isa");
    isa.frontend_config()
}

fn wrap<T: pliron::op::Op>(op_ptr: Ptr<Operation>, ctx: &pliron::context::Context) -> T {
    Operation::get_op_dyn(op_ptr, ctx)
        .downcast::<T>()
        .expect("expected op type")
}

/// Lower a pliron [`FragmentIr`] function into a Cranelift [`Function`].
pub fn lower_to_clif(ir: &FragmentIr) -> Function {
    let ctx = &*ir.ctx;
    let func_op = ir.func.deref(ctx);
    let func_llvm: FuncOp = Operation::get_op_dyn(ir.func, ctx)
        .downcast::<FuncOp>()
        .expect("not a FuncOp");
    let fn_ty = func_llvm.get_type(ctx);
    let fn_ty_ref = fn_ty.deref(ctx);
    // Build signature: SystemV / standard callconv, (i64, i64, i64) -> i8
    let mut sig = Signature::new(CallConv::SystemV);
    for arg_ty in fn_ty_ref.arg_types() {
        sig.params.push(AbiParam::new(lower_type(ctx, arg_ty)));
    }
    let res_ty = fn_ty_ref.result_type();
    sig.returns.push(AbiParam::new(lower_type(ctx, res_ty)));

    let mut clif_fn =
        Function::with_name_signature(cranelift_codegen::ir::UserFuncName::user(0, 0), sig);

    let mut fn_builder_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut clif_fn, &mut fn_builder_ctx);

    // 1. Create a Cranelift Block for each pliron BasicBlock in region 0
    let mut block_map: HashMap<Ptr<pliron::basic_block::BasicBlock>, Block> = HashMap::new();
    let region = func_op.get_region(0);
    use pliron::linked_list::ContainsLinkedList;
    for blk_ptr in region.deref(ctx).iter(ctx) {
        let clif_blk = b.create_block();
        block_map.insert(blk_ptr, clif_blk);
    }

    // 2. Map block arguments / entry parameters
    let mut val_map: HashMap<PlironValue, ClifValue> = HashMap::new();
    let entry_blk = func_llvm.get_entry_block(ctx).expect("entry block missing");
    let clif_entry = block_map[&entry_blk];

    b.append_block_params_for_function_params(clif_entry);
    b.switch_to_block(clif_entry);

    // Bind entry block arguments from Cranelift params
    for (i, p_val) in entry_blk.deref(ctx).arguments().enumerate() {
        let c_val = b.block_params(clif_entry)[i];
        val_map.insert(p_val, c_val);
    }

    // For other blocks, append typed block parameters
    for blk_ptr in region.deref(ctx).iter(ctx) {
        if blk_ptr == entry_blk {
            continue;
        }
        let clif_blk = block_map[&blk_ptr];
        for p_val in blk_ptr.deref(ctx).arguments() {
            let ty = lower_type(ctx, p_val.get_type(ctx));
            let c_val = b.append_block_param(clif_blk, ty);
            val_map.insert(p_val, c_val);
        }
    }

    // 3. Lower instructions block by block
    for blk_ptr in region.deref(ctx).iter(ctx) {
        let clif_blk = block_map[&blk_ptr];
        b.switch_to_block(clif_blk);

        for op_ptr in blk_ptr.deref(ctx).iter(ctx) {
            let op = op_ptr.deref(ctx);
            let op_id = Operation::get_opid(op_ptr, ctx);
            let op_name = alloc::format!("{}", op_id.disp(ctx));

            match op_name.as_str() {
                "llvm.constant" => {
                    let cop = crate::lowering::wrap::<ConstantOp>(op_ptr, ctx);
                    let res_val = cop.get_result(ctx);
                    let raw_attr = cop
                        .get_attr_llvm_constant_value(ctx)
                        .expect("constant value attr");
                    let clif_val = if let Some(fa) =
                        raw_attr.downcast_ref::<pliron::builtin::attributes::FPSingleAttr>()
                    {
                        b.ins()
                            .f32const(pliron::utils::apfloat::single_to_f32(fa.0))
                    } else if let Some(ia) =
                        raw_attr.downcast_ref::<pliron::builtin::attributes::IntegerAttr>()
                    {
                        let width = ia.get_type().deref(ctx).width();
                        let raw = ia.value().to_u64();
                        match width {
                            1 | 8 => b.ins().iconst(types::I8, raw as i64),
                            32 => b.ins().iconst(types::I32, raw as i64),
                            64 => b.ins().iconst(types::I64, raw as i64),
                            _ => panic!("unhandled const width {width}"),
                        }
                    } else {
                        panic!("unsupported constant attribute");
                    };
                    val_map.insert(res_val, clif_val);
                }
                "llvm.load" => {
                    let lop = crate::lowering::wrap::<LoadOp>(op_ptr, ctx);
                    let addr = val_map[&op.get_operand(0)];
                    let res_val = lop.get_result(ctx);
                    let ty = lower_type(ctx, res_val.get_type(ctx));
                    let loaded =
                        b.ins()
                            .load(ty, cranelift_codegen::ir::MemFlagsData::trusted(), addr, 0);
                    val_map.insert(res_val, loaded);
                }
                "llvm.store" => {
                    let val = val_map[&op.get_operand(0)];
                    let addr = val_map[&op.get_operand(1)];
                    b.ins()
                        .store(cranelift_codegen::ir::MemFlagsData::trusted(), val, addr, 0);
                }
                "llvm.getelementptr" => {
                    let gep = crate::lowering::wrap::<GetElementPtrOp>(op_ptr, ctx);
                    let base = val_map[&op.get_operand(0)];
                    let mut cur = base;
                    for idx in gep.indices(ctx) {
                        match idx {
                            pliron_llvm::ops::GepIndex::Constant(c) => {
                                if c != 0 {
                                    cur = b.ins().iadd_imm(cur, c as i64);
                                }
                            }
                            pliron_llvm::ops::GepIndex::Value(v) => {
                                let offset = val_map[&v];
                                cur = b.ins().iadd(cur, offset);
                            }
                        }
                    }
                    val_map.insert(gep.get_result(ctx), cur);
                }
                "llvm.fadd" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().fadd(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.fsub" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().fsub(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.fmul" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().fmul(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.fdiv" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().fdiv(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.add" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().iadd(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.sub" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().isub(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.mul" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().imul(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.and" => {
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    let res = b.ins().band(a, c);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.fcmp" => {
                    let fcmp = crate::lowering::wrap::<FCmpOp>(op_ptr, ctx);
                    let pred = fcmp.get_attr_llvm_fcmp_predicate(ctx).unwrap();
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    use cranelift_codegen::ir::condcodes::FloatCC;
                    let cc = match *pred {
                        FCmpPredicateAttr::OEQ => FloatCC::Equal,
                        FCmpPredicateAttr::OGT => FloatCC::GreaterThan,
                        FCmpPredicateAttr::OGE => FloatCC::GreaterThanOrEqual,
                        FCmpPredicateAttr::OLT => FloatCC::LessThan,
                        FCmpPredicateAttr::OLE => FloatCC::LessThanOrEqual,
                        FCmpPredicateAttr::ONE => FloatCC::NotEqual,
                        FCmpPredicateAttr::True => FloatCC::Ordered,
                        _ => FloatCC::Ordered,
                    };
                    let res = b.ins().fcmp(cc, a, c);
                    val_map.insert(fcmp.get_result(ctx), res);
                }
                "llvm.icmp" => {
                    let icmp = crate::lowering::wrap::<ICmpOp>(op_ptr, ctx);
                    let pred = icmp.get_attr_llvm_icmp_predicate(ctx).unwrap();
                    let a = val_map[&op.get_operand(0)];
                    let c = val_map[&op.get_operand(1)];
                    use cranelift_codegen::ir::condcodes::IntCC;
                    let cc = match *pred {
                        ICmpPredicateAttr::EQ => IntCC::Equal,
                        ICmpPredicateAttr::NE => IntCC::NotEqual,
                        ICmpPredicateAttr::SLT => IntCC::SignedLessThan,
                        ICmpPredicateAttr::SGT => IntCC::SignedGreaterThan,
                        ICmpPredicateAttr::ULT => IntCC::UnsignedLessThan,
                        ICmpPredicateAttr::UGT => IntCC::UnsignedGreaterThan,
                        _ => IntCC::Equal,
                    };
                    let res = b.ins().icmp(cc, a, c);
                    val_map.insert(icmp.get_result(ctx), res);
                }
                "llvm.select" => {
                    let cond = val_map[&op.get_operand(0)];
                    let t_val = val_map[&op.get_operand(1)];
                    let f_val = val_map[&op.get_operand(2)];
                    let b_cond = if b.func.dfg.value_type(cond) == types::I8 {
                        let zero = b.ins().iconst(types::I8, 0);
                        b.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            cond,
                            zero,
                        )
                    } else {
                        panic!("unsupported select cond type");
                    };
                    let res = b.ins().select(b_cond, t_val, f_val);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.fptoui" => {
                    let v = val_map[&op.get_operand(0)];
                    let res_ty = lower_type(ctx, op.get_result(0).get_type(ctx));
                    let res = b.ins().fcvt_to_uint(res_ty, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.fptosi" => {
                    let v = val_map[&op.get_operand(0)];
                    let res_ty = lower_type(ctx, op.get_result(0).get_type(ctx));
                    let res = b.ins().fcvt_to_sint(res_ty, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.uitofp" => {
                    let v = val_map[&op.get_operand(0)];
                    let res = b.ins().fcvt_from_uint(types::F32, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.sitofp" => {
                    let v = val_map[&op.get_operand(0)];
                    let res = b.ins().fcvt_from_sint(types::F32, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.zext" => {
                    let v = val_map[&op.get_operand(0)];
                    let res_ty = lower_type(ctx, op.get_result(0).get_type(ctx));
                    let res = b.ins().uextend(res_ty, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.sext" => {
                    let v = val_map[&op.get_operand(0)];
                    let res_ty = lower_type(ctx, op.get_result(0).get_type(ctx));
                    let res = b.ins().sextend(res_ty, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.trunc" => {
                    let v = val_map[&op.get_operand(0)];
                    let res_ty = lower_type(ctx, op.get_result(0).get_type(ctx));
                    let res = b.ins().ireduce(res_ty, v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.bitcast" => {
                    let v = val_map[&op.get_operand(0)];
                    let res_ty = lower_type(ctx, op.get_result(0).get_type(ctx));
                    let res =
                        b.ins()
                            .bitcast(res_ty, cranelift_codegen::ir::MemFlagsData::trusted(), v);
                    val_map.insert(op.get_result(0), res);
                }
                "llvm.br" => {
                    let br = crate::lowering::wrap::<BrOp>(op_ptr, ctx);
                    let dest_ptr = op.get_successor(0);
                    let dest = block_map[&dest_ptr];
                    let args: Vec<cranelift_codegen::ir::BlockArg> = br
                        .successor_operands(ctx, 0)
                        .iter()
                        .map(|v| val_map[v].into())
                        .collect();
                    b.ins().jump(dest, &args);
                }
                "llvm.cond_br" => {
                    let cbr = crate::lowering::wrap::<CondBrOp>(op_ptr, ctx);
                    let cond = val_map[&op.get_operand(0)];
                    let t_dest = block_map[&op.get_successor(0)];
                    let t_args: Vec<cranelift_codegen::ir::BlockArg> = cbr
                        .successor_operands(ctx, 0)
                        .iter()
                        .map(|v| val_map[v].into())
                        .collect();
                    let f_dest = block_map[&op.get_successor(1)];
                    let f_args: Vec<cranelift_codegen::ir::BlockArg> = cbr
                        .successor_operands(ctx, 1)
                        .iter()
                        .map(|v| val_map[v].into())
                        .collect();

                    let b_cond = if b.func.dfg.value_type(cond) == types::I8 {
                        let zero = b.ins().iconst(types::I8, 0);
                        b.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            cond,
                            zero,
                        )
                    } else {
                        panic!("unsupported cond_br cond type");
                    };
                    b.ins().brif(b_cond, t_dest, &t_args, f_dest, &f_args);
                }
                "llvm.return" => {
                    let rop = crate::lowering::wrap::<ReturnOp>(op_ptr, ctx);
                    let val = rop.retval(ctx).map(|v| val_map[&v]);
                    if let Some(v) = val {
                        b.ins().return_(&[v]);
                    } else {
                        b.ins().return_(&[]);
                    }
                }
                "llvm.call" => {
                    // For now, external calls like @vantage_expf are lowered as an unsupported placeholder
                    // or trap in no_std / interpreter context until JIT linker is wired.
                    let res_val = op.get_result(0);
                    let res_ty = lower_type(ctx, res_val.get_type(ctx));
                    let dummy = b.ins().f32const(1.0);
                    val_map.insert(res_val, dummy);
                }
                other => panic!("unhandled pliron op in lowering: {other}"),
            }
        }
    }

    b.seal_all_blocks();
    b.finalize(default_frontend_config());
    clif_fn
}
