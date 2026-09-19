//! Real pliron SSA construction for fragment programs.
//!
//! Every [`FragmentKey`] compiles to one `llvm.func @frag_entry` in a
//! pliron [`Context`], using the LLVM dialect as the target IR. The
//! structure mirrors the reference evaluator in `lib.rs` op-for-op so the
//! parity tests can demand byte-exact agreement on finite inputs:
//!
//! - tex units enabled by the key get an inlined sample region (bounds
//!   checks via blocks/phis; wrap mode, filter and format expansion are
//!   runtime selects, matching `sample_tex`'s branchless-equivalent math).
//! - texenv mode, fog mode and alpha func are *keyed*, so only the taken
//!   expression is emitted.
//! - `expf` (fog modes 2/3) is emitted as a call to the external
//!   `vantage_expf`; backends either link it or reject the program.
//!
//! Known parity edge: NaN colors reaching the output pack take the
//! `fptoui`-undefined path (the Rust reference saturates to 0). Finite
//! inputs — everything a rasterizer legitimately produces — match exactly.

use crate::offsets;
use crate::FragmentKey;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use hashbrown::HashMap;
use pliron::builtin::attributes::{FPSingleAttr, IntegerAttr};
use pliron::builtin::op_interfaces::CallOpCallable;
use pliron::builtin::op_interfaces::{OneResultInterface, SingleBlockRegionInterface};
use pliron::builtin::types::{FP32Type, IntegerType, Signedness};
use pliron::context::{Context, Ptr};
use pliron::identifier::Identifier;
use pliron::irbuild::inserter::{IRInserter, Inserter};
use pliron::irbuild::listener::DummyListener;
use pliron::op::Op;
use pliron::operation::Operation;
use pliron::printable::Printable;
use pliron::r#type::{Type, Typed};
use pliron::utils::apfloat::f32_to_single;
use pliron::utils::apint::APInt;
use pliron::value::Value;
use pliron_llvm::attributes::{FCmpPredicateAttr, FastmathFlagsAttr, ICmpPredicateAttr};
use pliron_llvm::op_interfaces::{BinArithOp, CastOpInterface, FastMathFlags};
use pliron_llvm::ops::{
    AddOp, AndOp, BitcastOp, BrOp, CallOp, CondBrOp, ConstantOp, FAddOp, FCmpOp, FDivOp, FMulOp,
    FPToSIOp, FPToUIOp, FSubOp, FuncOp, GetElementPtrOp, ICmpOp, LoadOp, MulOp, ReturnOp, SExtOp,
    SIToFPOp, SelectOp, StoreOp, SubOp, TruncOp, UIToFPOp, ZExtOp,
};
use pliron_llvm::types::{FuncType, PointerType};
use vantage_raster::gl;

/// A compiled pliron fragment program: the owning arena plus its roots.
///
/// `Ptr` handles are stable slotmap keys, so the self-reference
/// (`ctx` owning what `module`/`func` point into) is sound; `Rc` keeps the
/// program shareable without `Clone`ing the arena.
pub struct FragmentIr {
    pub ctx: Rc<Context>,
    pub module: Ptr<Operation>,
    pub func: Ptr<Operation>,
}

const EXPF_NAME: &str = "vantage_expf";

/// Immutable per-texture-unit context for the inlined `fetch` helper.
#[derive(Clone, Copy)]
struct TexCtx {
    data: Value,
    dlen: Value,
    w: Value,
    h: Value,
    mregion: Ptr<pliron::region::Region>,
}

/// Byte-addressing GEP element type.
fn i8t(ctx: &Context) -> pliron::r#type::TypeHandle {
    IntegerType::get(ctx, 8, Signedness::Signless).to_handle()
}
fn i1t(ctx: &Context) -> pliron::r#type::TypeHandle {
    IntegerType::get(ctx, 1, Signedness::Signless).to_handle()
}
fn i32t(ctx: &Context) -> pliron::r#type::TypeHandle {
    IntegerType::get(ctx, 32, Signedness::Signless).to_handle()
}
fn i64t(ctx: &Context) -> pliron::r#type::TypeHandle {
    IntegerType::get(ctx, 64, Signedness::Signless).to_handle()
}
fn f32t(ctx: &Context) -> pliron::r#type::TypeHandle {
    FP32Type::get(ctx).to_handle()
}
fn ptr(ctx: &Context) -> pliron::r#type::TypeHandle {
    PointerType::get(ctx, 0).to_handle()
}

/// Builder state: one insertion cursor + uniqued constants.
struct B<'c> {
    ctx: &'c mut Context,
    ins: IRInserter<DummyListener>,
    c_f32: HashMap<u32, Value>,
    c_i64: HashMap<u64, Value>,
    c_i8: HashMap<u8, Value>,
    c_i32: HashMap<u32, Value>,
}

/// Append an op and return its first result. `ctor` receives `&mut Context`.
macro_rules! emit {
    ($b:expr, $ctor:expr) => {{
        let b = &mut *$b;
        let op = ($ctor)(&mut *b.ctx);
        let ptr = op.get_operation();
        let c: &Context = &*b.ctx;
        b.ins.append_op(c, &op);
        ptr.deref(c).get_result(0)
    }};
}

macro_rules! const_int {
    ($selfx:expr, $field:ident, $width:literal, $v:expr, $mk:ident) => {{
        let b = &mut *$selfx;
        if let Some(x) = b.$field.get(&$v) {
            *x
        } else {
            let v = emit!(b, |ctx: &mut Context| ConstantOp::new(
                ctx,
                Box::new(IntegerAttr::new(
                    IntegerType::get(ctx, $width, Signedness::Signless),
                    APInt::$mk($v, core::num::NonZeroUsize::new($width).unwrap()),
                )),
            ));
            b.$field.insert($v, v);
            v
        }
    }};
}

impl<'c> B<'c> {
    fn new(ctx: &'c mut Context, entry: Ptr<pliron::basic_block::BasicBlock>) -> Self {
        Self {
            ctx,
            ins: IRInserter::new_at_block_end(entry),
            c_f32: Default::default(),
            c_i64: Default::default(),
            c_i8: Default::default(),
            c_i32: Default::default(),
        }
    }

    fn at_block(&mut self, blk: Ptr<pliron::basic_block::BasicBlock>) {
        self.ins = IRInserter::new_at_block_end(blk);
    }

    fn t_f32(&mut self) -> pliron::r#type::TypeHandle {
        f32t(&*self.ctx)
    }
    fn t_i8(&mut self) -> pliron::r#type::TypeHandle {
        i8t(&*self.ctx)
    }
    fn t_ptr(&mut self) -> pliron::r#type::TypeHandle {
        ptr(&*self.ctx)
    }

    /// Reborrow the context (avoids moving out of `self.ctx`).
    fn cx(&mut self) -> &mut Context {
        &mut *self.ctx
    }

    fn fc(&mut self, bits: u32) -> Value {
        if let Some(x) = self.c_f32.get(&bits) {
            return *x;
        }
        let v = emit!(self, |ctx: &mut Context| ConstantOp::new(
            ctx,
            Box::new(FPSingleAttr(f32_to_single(f32::from_bits(bits)))),
        ));
        self.c_f32.insert(bits, v);
        v
    }
    fn f(&mut self, x: f32) -> Value {
        self.fc(x.to_bits())
    }
    fn i8(&mut self, v: u8) -> Value {
        const_int!(self, c_i8, 8, v, from_u8)
    }
    fn i32(&mut self, v: u32) -> Value {
        const_int!(self, c_i32, 32, v, from_u32)
    }
    fn i64(&mut self, v: u64) -> Value {
        const_int!(self, c_i64, 64, v, from_u64)
    }

    // ---- loads / stores -------------------------------------------------

    fn ld(&mut self, ty: pliron::r#type::TypeHandle, addr: Value) -> Value {
        emit!(self, |ctx: &mut Context| LoadOp::new(ctx, addr, ty))
    }
    fn ld_f32(&mut self, addr: Value) -> Value {
        let t = f32t(self.ctx);
        self.ld(t, addr)
    }
    fn ld_u8(&mut self, addr: Value) -> Value {
        let t = i8t(self.ctx);
        self.ld(t, addr)
    }
    fn ld_i32(&mut self, addr: Value) -> Value {
        let t = i32t(self.ctx);
        self.ld(t, addr)
    }
    fn ld_i64(&mut self, addr: Value) -> Value {
        let t = i64t(self.ctx);
        self.ld(t, addr)
    }
    fn ld_ptr(&mut self, addr: Value) -> Value {
        let t = ptr(self.ctx);
        self.ld(t, addr)
    }
    fn st(&mut self, val: Value, addr: Value) {
        let op = StoreOp::new(&mut *self.ctx, val, addr);
        self.ins.append_op(&*self.ctx, &op);
    }

    /// `base + byte_off` (constant).
    fn gepc(&mut self, base: Value, byte_off: u32) -> Value {
        let t = i8t(&*self.ctx);
        emit!(self, |ctx: &mut Context| GetElementPtrOp::new(
            ctx,
            base,
            vec![pliron_llvm::ops::GepIndex::Constant(byte_off)],
            t,
        ))
    }
    /// `base + idx` where idx is an i64 value (bytes, elem type i8).
    fn gepe(&mut self, base: Value, idx: Value) -> Value {
        let t = i8t(&*self.ctx);
        emit!(self, |ctx: &mut Context| GetElementPtrOp::new(
            ctx,
            base,
            vec![pliron_llvm::ops::GepIndex::Value(idx)],
            t,
        ))
    }

    // ---- float / int arithmetic -----------------------------------------

    fn fbin<T: BinArithOp + FastMathFlags>(&mut self, a: Value, b: Value) -> Value {
        emit!(self, |ctx: &mut Context| {
            let op = T::new(ctx, a, b);
            op.set_fast_math_flags(ctx, FastmathFlagsAttr::default());
            op
        })
    }
    fn fadd(&mut self, a: Value, b: Value) -> Value {
        self.fbin::<FAddOp>(a, b)
    }
    fn fsub(&mut self, a: Value, b: Value) -> Value {
        self.fbin::<FSubOp>(a, b)
    }
    fn fmul(&mut self, a: Value, b: Value) -> Value {
        self.fbin::<FMulOp>(a, b)
    }
    fn fdiv(&mut self, a: Value, b: Value) -> Value {
        self.fbin::<FDivOp>(a, b)
    }

    fn ibin<T: BinArithOp>(&mut self, a: Value, b: Value) -> Value {
        emit!(self, |ctx: &mut Context| T::new(ctx, a, b))
    }
    fn iadd(&mut self, a: Value, b: Value) -> Value {
        self.ibin::<AddOp>(a, b)
    }
    fn isub(&mut self, a: Value, b: Value) -> Value {
        self.ibin::<SubOp>(a, b)
    }
    fn imul(&mut self, a: Value, b: Value) -> Value {
        self.ibin::<MulOp>(a, b)
    }

    fn fcmp(&mut self, pred: FCmpPredicateAttr, a: Value, b: Value) -> Value {
        emit!(self, |ctx: &mut Context| FCmpOp::new(ctx, pred, a, b))
    }
    fn icmp(&mut self, pred: ICmpPredicateAttr, a: Value, b: Value) -> Value {
        emit!(self, |ctx: &mut Context| ICmpOp::new(ctx, pred, a, b))
    }
    fn sel(&mut self, c: Value, t: Value, f: Value) -> Value {
        emit!(self, |ctx: &mut Context| SelectOp::new(ctx, c, t, f))
    }
    fn cast<T: CastOpInterface>(&mut self, v: Value, ty: pliron::r#type::TypeHandle) -> Value {
        emit!(self, |ctx: &mut Context| T::new(ctx, v, ty))
    }
    fn fptosi32(&mut self, v: Value) -> Value {
        let t = i32t(self.ctx);
        self.cast::<FPToSIOp>(v, t)
    }
    fn fptoui32(&mut self, v: Value) -> Value {
        let t = i32t(self.ctx);
        self.cast::<FPToUIOp>(v, t)
    }
    fn fptoui64(&mut self, v: Value) -> Value {
        let t = i64t(self.ctx);
        self.cast::<FPToUIOp>(v, t)
    }
    fn uitofp32(&mut self, v: Value) -> Value {
        let t = f32t(self.ctx);
        self.cast::<UIToFPOp>(v, t)
    }
    fn sext32_64(&mut self, v: Value) -> Value {
        let t = i64t(self.ctx);
        self.cast::<SExtOp>(v, t)
    }
    fn zext32_64(&mut self, v: Value) -> Value {
        let t = i64t(self.ctx);
        self.cast::<ZExtOp>(v, t)
    }
    fn trunc32_8(&mut self, v: Value) -> Value {
        let t = i8t(self.ctx);
        self.cast::<TruncOp>(v, t)
    }
    fn trunc64_32(&mut self, v: Value) -> Value {
        let t = i32t(self.ctx);
        self.cast::<TruncOp>(v, t)
    }
    fn bitcast64_64(&mut self, v: Value) -> Value {
        let t = i64t(self.ctx);
        self.cast::<BitcastOp>(v, t)
    }
    fn sitofp(&mut self, v: Value, ty: pliron::r#type::TypeHandle) -> Value {
        self.cast::<SIToFPOp>(v, ty)
    }

    /// `floor(x)` as f32, branchless: t = trunc(x); t + (x < t ? -1 : 0).
    fn floor_f32(&mut self, x: Value) -> Value {
        let t32 = self.fptosi32(x);
        let tf = {
            let t = f32t(self.ctx);
            self.sitofp(t32, t)
        };
        let below = self.fcmp(FCmpPredicateAttr::OLT, x, tf);
        let one = self.f(1.0);
        let z = self.f(0.0);
        let corr = self.sel(below, one, z);
        self.fsub(tf, corr)
    }

    /// `floorf(x) as i64` — Rust's saturating fptosi. Clamp to i64 range
    /// first so NaN/out-of-range never hits the undefined path.
    fn floor_i64_sat(&mut self, x: Value) -> Value {
        let t = self.floor_f32(x);
        let i64_min = self.f(f32::from_bits(0xCF000000)); // -9223372036854775808.0
        let i64_max = self.f(f32::from_bits(0x4F000000)); //  9223372036854775808.0 (exclusive bound ok)
        let lo = self.fcmp(FCmpPredicateAttr::OLT, t, i64_min);
        let t = self.sel(lo, i64_min, t);
        let hi = self.fcmp(FCmpPredicateAttr::OGE, t, i64_max);
        let maxf = self.f(f32::from_bits(0x4EFFFFFF));
        let t = self.sel(hi, maxf, t);
        self.cast::<FPToSIOp>(t, i64t(self.ctx))
    }

    /// clamp(x, 0, 1) with Rust's NaN-preserving `clamp` semantics.
    fn clamp01(&mut self, x: Value) -> Value {
        let z = self.f(0.0);
        let o = self.f(1.0);
        let lo = self.fcmp(FCmpPredicateAttr::OLT, x, z);
        let m = self.sel(lo, z, x);
        let hi = self.fcmp(FCmpPredicateAttr::OGT, m, o);
        self.sel(hi, o, m)
    }

    // ---- control flow ----------------------------------------------------

    fn cond_br(
        &mut self,
        c: Value,
        t: Ptr<pliron::basic_block::BasicBlock>,
        targs: Vec<Value>,
        f: Ptr<pliron::basic_block::BasicBlock>,
        fargs: Vec<Value>,
    ) {
        let op = CondBrOp::new(&mut *self.ctx, c, t, targs, f, fargs);
        self.ins.append_op(&*self.ctx, &op);
    }
    fn br(&mut self, d: Ptr<pliron::basic_block::BasicBlock>, args: Vec<Value>) {
        let op = BrOp::new(&mut *self.ctx, d, args);
        self.ins.append_op(&*self.ctx, &op);
    }
    fn ret(&mut self, v: Value) {
        let op = ReturnOp::new(&mut *self.ctx, Some(v));
        self.ins.append_op(&*self.ctx, &op);
    }
    fn block(
        &mut self,
        arg_tys: Vec<pliron::r#type::TypeHandle>,
        parent: Ptr<pliron::region::Region>,
    ) -> Ptr<pliron::basic_block::BasicBlock> {
        let ctx = &mut *self.ctx;
        let blk = pliron::basic_block::BasicBlock::new(ctx, None, arg_tys);
        blk.insert_at_back(parent, ctx);
        blk
    }
}

/// Build the full fragment program for `key`.
pub fn build_fragment_program(key: &FragmentKey) -> FragmentIr {
    let mut ctx = Context::new();
    let module = pliron::builtin::ops::ModuleOp::new(&mut ctx, Identifier::new("fragmod"));
    let body = module.get_body(&ctx, 0);
    let mregion = body.deref(&ctx).get_parent_region().unwrap();

    // frag_entry(state: ptr, varyings: ptr, out: ptr) -> i8 (bool ABI)
    let fty = FuncType::get(
        &ctx,
        i8t(&ctx),
        vec![ptr(&ctx), ptr(&ctx), ptr(&ctx)],
        false,
    );
    let func = FuncOp::new(&mut ctx, Identifier::new("frag_entry"), fty);
    func.get_operation().insert_at_back(body, &mut ctx);

    // external expf declaration (only when fog modes 2/3 need it)
    if matches!(key.fog_mode, 2 | 3) {
        let ety = FuncType::get(&ctx, f32t(&ctx), vec![f32t(&ctx)], false);
        let decl = FuncOp::new(&mut ctx, Identifier::new(EXPF_NAME), ety);
        // FuncOp::new creates no region: a declaration by construction.
        decl.get_operation().insert_at_back(body, &mut ctx);
    }

    let entry = func.get_or_create_entry_block(&mut ctx);
    let args: Vec<Value> = entry.deref(&ctx).arguments().collect();
    let (p_state, p_var, p_out) = (args[0], args[1], args[2]);

    let mut b = B::new(&mut ctx, entry);

    // ---- load varyings ---------------------------------------------------
    let mut color = [
        {
            let a = b.gepc(p_var, offsets::varyings::COLOR);
            b.ld_f32(a)
        },
        {
            let a = b.gepc(p_var, offsets::varyings::COLOR + 4);
            b.ld_f32(a)
        },
        {
            let a = b.gepc(p_var, offsets::varyings::COLOR + 8);
            b.ld_f32(a)
        },
        {
            let a = b.gepc(p_var, offsets::varyings::COLOR + 12);
            b.ld_f32(a)
        },
    ];
    let texc = [
        [
            {
                let a = b.gepc(p_var, offsets::varyings::TEX0);
                b.ld_f32(a)
            },
            {
                let a = b.gepc(p_var, offsets::varyings::TEX0 + 4);
                b.ld_f32(a)
            },
        ],
        [
            {
                let a = b.gepc(p_var, offsets::varyings::TEX1);
                b.ld_f32(a)
            },
            {
                let a = b.gepc(p_var, offsets::varyings::TEX1 + 4);
                b.ld_f32(a)
            },
        ],
    ];
    let v_fog = {
        let a = b.gepc(p_var, offsets::varyings::FOG);
        b.ld_f32(a)
    };

    // ---- per-unit sample + texenv ----------------------------------------
    for unit in 0..2usize {
        if !key.tex_enabled[unit] {
            continue;
        }
        let texbase = b.gepc(
            p_state,
            offsets::state::TEXTURES + (unit as u32) * offsets::state::TEXTURE_STRIDE,
        );

        // Runtime validity: !enabled || w==0 || h==0 || data==null -> default
        let en = {
            let a = b.gepc(texbase, offsets::tex::ENABLED);
            b.ld_u8(a)
        };
        let w = {
            let a = b.gepc(texbase, offsets::tex::WIDTH);
            b.ld_i32(a)
        };
        let h = {
            let a = b.gepc(texbase, offsets::tex::HEIGHT);
            b.ld_i32(a)
        };
        let data = {
            let a = b.gepc(texbase, offsets::tex::DATA);
            b.ld_ptr(a)
        };
        let dlen = {
            let a = b.gepc(texbase, offsets::tex::DATA_LEN);
            b.ld_i64(a)
        };
        let fmt = {
            let a = b.gepc(texbase, offsets::tex::FORMAT);
            b.ld_i32(a)
        };
        let wraps = {
            let a = b.gepc(texbase, offsets::tex::WRAP_S);
            b.ld_i32(a)
        };
        let wart = {
            let a = b.gepc(texbase, offsets::tex::WRAP_T);
            b.ld_i32(a)
        };
        let magf = {
            let a = b.gepc(texbase, offsets::tex::MAG_FILTER);
            b.ld_i32(a)
        };

        let zero8 = b.i8(0);
        let zero32 = b.i32(0);
        let nullp = {
            let z = b.i64(0);
            let d = b.bitcast64_64(data);
            b.icmp(ICmpPredicateAttr::EQ, d, z)
        };
        let bad_en = b.icmp(ICmpPredicateAttr::EQ, en, zero8);
        let bad_w = b.icmp(ICmpPredicateAttr::EQ, w, zero32);
        let bad_h = b.icmp(ICmpPredicateAttr::EQ, h, zero32);
        // i1 or-chain via select (keeps everything branchless)
        let t_true = b.i8(1);
        let or_en = b.sel(bad_en, t_true, zero8);
        let or_w = b.sel(bad_w, t_true, or_en);
        let or_h = b.sel(bad_h, t_true, or_w);
        let or_n = b.sel(nullp, t_true, or_h);
        let bad = b.icmp(ICmpPredicateAttr::NE, or_n, zero8);

        let calc_blk = b.block(vec![], mregion);
        let tf = b.t_f32();
        let join_blk = b.block(vec![tf, tf, tf, tf], mregion);
        {
            let z = b.f(0.0);
            let o = b.f(1.0);
            b.cond_br(bad, join_blk, vec![z, z, z, o], calc_blk, vec![]);
        }

        // ---- calc: sampling ------------------------------------------------
        b.at_block(calc_blk);
        let wa = wrap_calc(&mut b, texc[unit][0], wraps);
        let wb = wrap_calc(&mut b, texc[unit][1], wart);
        let w_f = {
            let w64 = b.zext32_64(w);
            {
                let tf = b.t_f32();
                b.sitofp(w64, tf)
            }
        };
        let h_f = {
            let h64 = b.zext32_64(h);
            {
                let tf = b.t_f32();
                b.sitofp(h64, tf)
            }
        };
        let fu = b.fmul(wa, w_f);
        let fv = b.fmul(wb, h_f);

        // fetch_texel(x i64, y i64) -> [f32;4] inlined via a helper block
        // taking (x, y) and returning through join-style block args.
        // We inline twice (nearest) or four times (bilinear) by cloning the
        // region with distinct constants — cheaper: a shared helper region
        // is overkill; emit an inline sequence per fetch.
        let texctx = TexCtx {
            data,
            dlen,
            w,
            h,
            mregion,
        };
        fn fetch(b: &mut B<'_>, t: &TexCtx, x: Value, y: Value) -> [Value; 4] {
            // x = clamp(x, 0, w-1); y = clamp(y, 0, h-1)
            let wm1 = {
                let w64 = b.zext32_64(t.w);
                {
                    let one = b.i64(1);
                    b.isub(w64, one)
                }
            };
            let hm1 = {
                let h64 = b.zext32_64(t.h);
                {
                    let one = b.i64(1);
                    b.isub(h64, one)
                }
            };
            let zl = b.i64(0);
            let xl = b.icmp(ICmpPredicateAttr::SLT, x, zl);
            let x = b.sel(xl, zl, x);
            let xg = b.icmp(ICmpPredicateAttr::SGT, x, wm1);
            let x = b.sel(xg, wm1, x);
            let yl = b.icmp(ICmpPredicateAttr::SLT, y, zl);
            let y = b.sel(yl, zl, y);
            let yg = b.icmp(ICmpPredicateAttr::SGT, y, hm1);
            let y = b.sel(yg, hm1, y);
            // o = (y * w + x) * 4
            let w64 = b.zext32_64(t.w);
            let row = b.imul(y, w64);
            let x64 = b.zext32_64(x);
            let px = b.iadd(row, x64);
            let four = b.i64(4);
            let o = b.imul(px, four);
            // bounds: o + 4 > dlen -> [0,0,0,1]
            let four = b.i64(4);
            let o4 = b.iadd(o, four);
            let over = b.icmp(ICmpPredicateAttr::UGT, o4, t.dlen);
            let bad_blk = b.block(vec![], t.mregion);
            let ti = b.t_i8();
            let ok_blk = b.block(vec![ti, ti, ti, ti], t.mregion);
            let tf = b.t_f32();
            let ret_blk = b.block(vec![tf, tf, tf, tf], t.mregion);
            b.cond_br(over, bad_blk, vec![], ok_blk, vec![]);
            b.at_block(bad_blk);
            let z = b.f(0.0);
            let o = b.f(1.0);
            b.br(ret_blk, vec![z, z, z, o]);
            b.at_block(ok_blk);
            let a0_addr = b.gepe(t.data, o);
            let a0 = b.ld_u8(a0_addr);
            let a1_base = b.gepc(t.data, 1);
            let a1_addr = b.gepe(a1_base, o);
            let a1 = b.ld_u8(a1_addr);
            let a2_base = b.gepc(t.data, 2);
            let a2_addr = b.gepe(a2_base, o);
            let a2 = b.ld_u8(a2_addr);
            let a3_base = b.gepc(t.data, 3);
            let a3_addr = b.gepe(a3_base, o);
            let a3 = b.ld_u8(a3_addr);
            b.br(ret_blk, vec![a0, a1, a2, a3]);
            b.at_block(ret_blk);
            let rargs: Vec<Value> = ret_blk.deref(&*b.cx()).arguments().collect();
            let s255 = b.f(255.0);
            let r_f = b.uitofp32(rargs[0]);
            let r = b.fdiv(r_f, s255);
            let g_f = b.uitofp32(rargs[1]);
            let g = b.fdiv(g_f, s255);
            let b_f = b.uitofp32(rargs[2]);
            let bb = b.fdiv(b_f, s255);
            let a_f = b.uitofp32(rargs[3]);
            let aa = b.fdiv(a_f, s255);
            [r, g, bb, aa]
        };

        let lin = {
            let nf = b.i32(gl::NEAREST);
            b.icmp(ICmpPredicateAttr::NE, magf, nf)
        };
        let tf = b.t_f32();
        let near_blk = b.block(vec![tf, tf, tf, tf], mregion);
        let tf = b.t_f32();
        let lin_blk = b.block(vec![tf, tf, tf, tf], mregion);
        let tf = b.t_f32();
        let samp_blk = b.block(vec![tf, tf, tf, tf], mregion);
        let half = b.f(0.5);
        b.cond_br(lin, lin_blk, vec![], near_blk, vec![]);

        b.at_block(near_blk);
        {
            let x = b.floor_i64_sat(fu);
            let y = b.floor_i64_sat(fv);
            let [r, g, bl, a] = fetch(&mut b, &texctx, x, y);
            b.br(samp_blk, vec![r, g, bl, a]);
        }
        b.at_block(lin_blk);
        {
            let a = b.fsub(fu, half);
            let x0 = b.floor_i64_sat(a);
            let a = b.fsub(fv, half);
            let y0 = b.floor_i64_sat(a);
            let tf = b.t_f32();
            let x0f = b.sitofp(x0, tf);
            let tf = b.t_f32();
            let y0f = b.sitofp(y0, tf);
            let a = b.fsub(fu, half);
            let tx = b.fsub(a, x0f);
            let a = b.fsub(fv, half);
            let ty = b.fsub(a, y0f);
            let one64 = b.i64(1);
            let [r00, g00, b00, a00] = fetch(&mut b, &texctx, x0, y0);
            let x1 = b.iadd(x0, one64);
            let [r10, g10, b10, a10] = fetch(&mut b, &texctx, x1, y0);
            let y1 = b.iadd(y0, one64);
            let [r01, g01, b01, a01] = fetch(&mut b, &texctx, x0, y1);
            let x1 = b.iadd(x0, one64);
            let y1 = b.iadd(y0, one64);
            let [r11, g11, b11, a11] = fetch(&mut b, &texctx, x1, y1);
            let mut out: [Option<Value>; 4] = [None, None, None, None];
            // lerp per channel: l0 = c00 + (c10-c00)*tx; l1 = c01 + (c11-c01)*tx; o = l0 + (l1-l0)*ty
            for (i, chans) in [
                [r00, r10, r01, r11],
                [g00, g10, g01, g11],
                [b00, b10, b01, b11],
                [a00, a10, a01, a11],
            ]
            .into_iter()
            .enumerate()
            {
                let [c00, c10, c01, c11] = chans;
                let d0 = b.fsub(c10, c00);
                let m0 = b.fmul(d0, tx);
                let l0 = b.fadd(c00, m0);
                let d1 = b.fsub(c11, c01);
                let m1 = b.fmul(d1, tx);
                let l1 = b.fadd(c01, m1);
                let d = b.fsub(l1, l0);
                let m = b.fmul(d, ty);
                out[i] = Some(b.fadd(l0, m));
            }
            let [r, g, bl, a] = out.map(|o| o.unwrap());
            b.br(samp_blk, vec![r, g, bl, a]);
        }

        // ---- samp: format expansion ---------------------------------------
        b.at_block(samp_blk);
        let sargs: Vec<Value> = samp_blk.deref(&*b.cx()).arguments().collect();
        let (sr, sg, sb, sa) = (sargs[0], sargs[1], sargs[2], sargs[3]);
        let lum = b.i32(gl::GL_LUMINANCE);
        let luma = b.i32(gl::GL_LUMINANCE_ALPHA);
        let is_lum = b.icmp(ICmpPredicateAttr::EQ, fmt, lum);
        let is_luma = b.icmp(ICmpPredicateAttr::EQ, fmt, luma);
        let fr_luma = b.sel(is_luma, sr, sr);
        let fr = b.sel(is_lum, sr, fr_luma);
        let fg_luma = b.sel(is_luma, sr, sg);
        let fg = b.sel(is_lum, sr, fg_luma);
        let fb_luma = b.sel(is_luma, sr, sb);
        let fb = b.sel(is_lum, sr, fb_luma);
        let one_f = b.f(1.0);
        let fa_luma = b.sel(is_luma, sa, sa);
        let fa = b.sel(is_lum, one_f, fa_luma);
        b.br(join_blk, vec![fr, fg, fb, fa]);

        // ---- join: texenv --------------------------------------------------
        b.at_block(join_blk);
        let jargs: Vec<Value> = join_blk.deref(&*b.cx()).arguments().collect();
        let tex = [jargs[0], jargs[1], jargs[2], jargs[3]];
        let one = b.f(1.0);
        color = match key.texenv_mode[unit] {
            gl::TEXENV_REPLACE => tex,
            gl::TEXENV_DECAL => {
                let a = tex[3];
                let oma = b.fsub(one, a);
                [
                    {
                        let p0 = b.fmul(color[0], oma);
                        let p1 = b.fmul(tex[0], a);
                        b.fadd(p0, p1)
                    },
                    {
                        let p0 = b.fmul(color[1], oma);
                        let p1 = b.fmul(tex[1], a);
                        b.fadd(p0, p1)
                    },
                    {
                        let p0 = b.fmul(color[2], oma);
                        let p1 = b.fmul(tex[2], a);
                        b.fadd(p0, p1)
                    },
                    color[3],
                ]
            }
            gl::TEXENV_BLEND => {
                let ebase = b.gepc(p_state, offsets::state::TEXENV_COLOR + (unit as u32) * 16);
                let env = [
                    {
                        let a = b.gepc(ebase, 0);
                        b.ld_f32(a)
                    },
                    {
                        let a = b.gepc(ebase, 4);
                        b.ld_f32(a)
                    },
                    {
                        let a = b.gepc(ebase, 8);
                        b.ld_f32(a)
                    },
                ];
                let omt0 = b.fsub(one, tex[0]);
                let omt1 = b.fsub(one, tex[1]);
                let omt2 = b.fsub(one, tex[2]);
                [
                    {
                        let p0 = b.fmul(color[0], omt0);
                        let p1 = b.fmul(env[0], tex[0]);
                        b.fadd(p0, p1)
                    },
                    {
                        let p0 = b.fmul(color[1], omt1);
                        let p1 = b.fmul(env[1], tex[1]);
                        b.fadd(p0, p1)
                    },
                    {
                        let p0 = b.fmul(color[2], omt2);
                        let p1 = b.fmul(env[2], tex[2]);
                        b.fadd(p0, p1)
                    },
                    b.fmul(color[3], tex[3]),
                ]
            }
            gl::TEXENV_ADD => {
                let s0 = b.fadd(color[0], tex[0]);
                let s1 = b.fadd(color[1], tex[1]);
                let s2 = b.fadd(color[2], tex[2]);
                let s3 = b.fmul(color[3], tex[3]);
                let m = |b: &mut B<'_>, v: Value| b.fcmp(FCmpPredicateAttr::OGT, v, one);
                let c0 = b.fcmp(FCmpPredicateAttr::OGT, s0, one);
                let c1 = b.fcmp(FCmpPredicateAttr::OGT, s1, one);
                let c2 = b.fcmp(FCmpPredicateAttr::OGT, s2, one);
                let c3 = b.fcmp(FCmpPredicateAttr::OGT, s3, one);
                [
                    b.sel(c0, one, s0),
                    b.sel(c1, one, s1),
                    b.sel(c2, one, s2),
                    b.sel(c3, one, s3),
                ]
            }
            _ => [
                b.fmul(tex[0], color[0]),
                b.fmul(tex[1], color[1]),
                b.fmul(tex[2], color[2]),
                b.fmul(tex[3], color[3]),
            ],
        };
    }

    // ---- fog ---------------------------------------------------------------
    if key.fog_mode != 0 {
        let fog = match key.fog_mode {
            1 => {
                let end = {
                    let a = b.gepc(p_state, offsets::state::FOG_END);
                    b.ld_f32(a)
                };
                let start = {
                    let a = b.gepc(p_state, offsets::state::FOG_START);
                    b.ld_f32(a)
                };
                {
                    let n = b.fsub(end, v_fog);
                    let d = b.fsub(end, start);
                    b.fdiv(n, d)
                }
            }
            2 => {
                let dens = {
                    let a = b.gepc(p_state, offsets::state::FOG_DENSITY);
                    b.ld_f32(a)
                };
                let arg = {
                    let m = b.fmul(dens, v_fog);
                    b.fneg(m)
                };
                b.call_expf(arg)
            }
            _ => {
                let dens = {
                    let a = b.gepc(p_state, offsets::state::FOG_DENSITY);
                    b.ld_f32(a)
                };
                let d = b.fmul(dens, v_fog);
                let dd = b.fmul(d, d);
                let arg = b.fneg(dd);
                b.call_expf(arg)
            }
        };
        let f = b.clamp01(fog);
        let one = b.f(1.0);
        let omt = b.fsub(one, f);
        let fbase = b.gepc(p_state, offsets::state::FOG_COLOR);
        for i in 0..3usize {
            let off = (i as u32) * 4;
            let a = b.gepc(fbase, off);
            let fc = b.ld_f32(a);
            {
                let p0 = b.fmul(color[i], f);
                let p1 = b.fmul(fc, omt);
                color[i] = b.fadd(p0, p1);
            }
        }
    }

    // ---- alpha test ---------------------------------------------------------
    let pass: Value = match key.alpha_func {
        gl::NEVER => b.i8(0),
        gl::ALWAYS => b.i8(1),
        f => {
            let r = {
                let a = b.gepc(p_state, offsets::state::ALPHA_REF);
                b.ld_f32(a)
            };
            let pred = match f {
                gl::LESS => FCmpPredicateAttr::OLT,
                gl::EQUAL => FCmpPredicateAttr::OEQ,
                gl::LEQUAL => FCmpPredicateAttr::OLE,
                gl::GREATER => FCmpPredicateAttr::OGT,
                gl::NOTEQUAL => FCmpPredicateAttr::ONE,
                gl::GEQUAL => FCmpPredicateAttr::OGE,
                _ => FCmpPredicateAttr::True,
            };
            let c = b.fcmp(pred, color[3], r);
            let t = b.i8(1);
            let z = b.i8(0);
            b.sel(c, t, z)
        }
    };

    // ---- out pack (BGRA) ----------------------------------------------------
    let z0 = b.i8(0);
    let discard = b.icmp(ICmpPredicateAttr::EQ, pass, z0);
    let ok_blk = b.block(vec![], mregion);
    let ti = b.t_i8();
    let end_blk = b.block(vec![ti], mregion);
    {
        let z = b.i8(0);
        b.cond_br(discard, end_blk, vec![z], ok_blk, vec![]);
    }
    b.at_block(ok_blk);
    let s255 = b.f(255.0);
    let half = b.f(0.5);
    fn pack(b: &mut B<'_>, s255: Value, half: Value, v: Value) -> Value {
        let c = b.clamp01(v);
        let m = b.fmul(c, s255);
        let t = b.fadd(m, half);
        let u = b.fptoui32(t);
        b.trunc32_8(u)
    }
    {
        let p0 = b.gepc(p_out, 0);
        let v = pack(&mut b, s255, half, color[2]);
        b.st(v, p0);
        let p1 = b.gepc(p_out, 1);
        let v = pack(&mut b, s255, half, color[1]);
        b.st(v, p1);
        let p2 = b.gepc(p_out, 2);
        let v = pack(&mut b, s255, half, color[0]);
        b.st(v, p2);
        let p3 = b.gepc(p_out, 3);
        let v = pack(&mut b, s255, half, color[3]);
        b.st(v, p3);
        let one = b.i8(1);
        b.br(end_blk, vec![one]);
    }
    b.at_block(end_blk);
    let eargs: Vec<Value> = end_blk.deref(&*b.cx()).arguments().collect();
    b.ret(eargs[0]);

    // verify every region we built (debug builds only: round-trip parse)
    let fop = func.get_operation();
    if cfg!(debug_assertions) {
        verify_all(&mut ctx, fop);
    }
    FragmentIr {
        ctx: Rc::new(ctx),
        module: module.get_operation(),
        func: fop,
    }
}

/// wrap coordinate per mode: REPEAT / CLAMP_TO_EDGE / MIRRORED_REPEAT.
fn wrap_calc(b: &mut B<'_>, x: Value, mode_val: Value) -> Value {
    let rep = b.i32(gl::REPEAT);
    let cte = b.i32(gl::CLAMP_TO_EDGE);
    let is_rep = b.icmp(ICmpPredicateAttr::EQ, mode_val, rep);
    let is_cte = b.icmp(ICmpPredicateAttr::EQ, mode_val, cte);

    // REPEAT: in-range fast path else fract-with-negative-fix
    let zero = b.f(0.0);
    let one = b.f(1.0);
    let ge0 = b.fcmp(FCmpPredicateAttr::OGE, x, zero);
    let lt1 = b.fcmp(FCmpPredicateAttr::OLT, x, one);
    let inr = b.and_i1(ge0, lt1);
    let fl = b.floor_f32(x);
    let fr = b.fsub(x, fl);
    let fneg = b.fcmp(FCmpPredicateAttr::OLT, fr, zero);
    let s = b.sel(fneg, one, zero);
    let fixed = b.fadd(fr, s);
    let repv = b.sel(inr, x, fixed);

    // CLAMP
    let clamped = b.clamp01(x);

    // MIRRORED: f = fract(x); (floor(x) even) ? f : 1 - f
    let fl2 = b.floor_f32(x);
    let fr2 = b.fsub(x, fl2);
    let inr2 = b.and_i1(ge0, lt1);
    let fneg2 = b.fcmp(FCmpPredicateAttr::OLT, fr2, zero);
    let s_fix = b.sel(fneg2, one, zero);
    let fr2_fix = b.fadd(fr2, s_fix);
    let fr2 = b.sel(inr2, fr2, fr2_fix);
    let fl64 = b.floor_i64_sat(x);
    let odd = {
        let one64 = b.i64(1);
        let o = b.and_i64(fl64, one64);
        {
            let z = b.i64(0);
            b.icmp(ICmpPredicateAttr::NE, o, z)
        }
    };
    let mir = {
        let sub = b.fsub(one, fr2);
        b.sel(odd, sub, fr2)
    };

    {
        let inner = b.sel(is_rep, repv, mir);
        b.sel(is_cte, clamped, inner)
    }
}

impl<'c> B<'c> {
    fn and_i1(&mut self, a: Value, b: Value) -> Value {
        emit!(self, |ctx: &mut Context| AndOp::new(ctx, a, b))
    }
    fn and_i64(&mut self, a: Value, b: Value) -> Value {
        emit!(self, |ctx: &mut Context| AndOp::new(ctx, a, b))
    }
    fn fneg(&mut self, v: Value) -> Value {
        let z = self.f(0.0);
        self.fsub(z, v)
    }
    fn call_expf(&mut self, arg: Value) -> Value {
        let tf = self.t_f32();
        let fty = FuncType::get(&*self.ctx, tf, vec![tf], false);
        emit!(self, |ctx: &mut Context| CallOp::new(
            ctx,
            CallOpCallable::Direct(Identifier::new(EXPF_NAME)),
            fty,
            vec![arg],
        ))
    }
}

/// Walk the function and verify every operation round-trips through the
/// LLVM text parser (catches malformed attributes/wrong operand kinds).
fn verify_all(ctx: &mut Context, fop: Ptr<Operation>) {
    use pliron::common_traits::Verify;
    use pliron::irfmt::parsers::spaced;
    use pliron::linked_list::ContainsLinkedList;
    use pliron::operation::Operation as OpStruct;
    use pliron::parsable::parse_from_str;
    let dump = {
        let c: &Context = ctx;
        let mut s = String::new();
        for region in fop.deref(c).regions() {
            for blk in region.deref(c).iter(c) {
                for op in blk.deref(c).iter(c) {
                    use core::fmt::Write;
                    let _ = write!(s, "{}\n", op.deref(c).disp(c));
                    s.push('\n');
                }
            }
        }
        s
    };
    for line in dump.lines() {
        if line.is_empty() {
            continue;
        }
        let reparsed = parse_from_str(spaced(OpStruct::top_level_parser()), ctx, line)
            .unwrap_or_else(|e| panic!("round-trip parse failed for {line}: {e}"));
        let c: &Context = ctx;
        reparsed
            .deref(c)
            .verify(c)
            .unwrap_or_else(|e| panic!("verify failed for {line}: {e}"));
    }
}
