//! Cranelift interpreter backend for Vantage Fragment programs (`no_std` compatible).

#![allow(dead_code)]

use crate::lowering::lower_to_clif;
use crate::program::FragmentIr;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use cranelift_codegen::data_value::DataValue;
use cranelift_codegen::ir::Function;
use cranelift_interpreter::environment::{FuncIndex, FunctionStore};
use cranelift_interpreter::interpreter::{Interpreter, InterpreterState};
use cranelift_interpreter::step::ControlFlow;
use vantage_raster::{FragState, Varyings};

/// Fragment program executor driven by the Cranelift interpreter.
pub struct InterpFragmentProgram {
    func: Function,
}

impl InterpFragmentProgram {
    /// Prepare a [`FragmentIr`] for execution via the Cranelift interpreter.
    pub fn compile(ir: &FragmentIr) -> Self {
        let clif_fn = lower_to_clif(ir, None);
        Self { func: clif_fn }
    }

    /// Evaluate the fragment function over raw pointers.
    /// Returns true if alpha test passed, false if discarded.
    pub unsafe fn evaluate(
        &self,
        state: *const FragState,
        varying: *const Varyings,
        out: *mut [u8; 4],
    ) -> bool {
        let func: &Function = &self.func;
        let mut store = FunctionStore::default();
        store.add("frag_entry".to_string(), func);
        let f_idx = store.index_of("frag_entry").expect("frag_entry registered");

        let interp_state = InterpreterState::default().with_function_store(store);
        let mut interp = Interpreter::new(interp_state);

        let args = vec![
            DataValue::I64(state as usize as i64),
            DataValue::I64(varying as usize as i64),
            DataValue::I64(out as usize as i64),
        ];

        match interp.call_by_index(f_idx, &args) {
            Ok(ControlFlow::Return(res)) => {
                if let Some(DataValue::I8(b)) = res.first() {
                    *b != 0
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
