//! Common intermediate representation shared by all language frontends.
//!
//! Frontends (Verilog, later SystemVerilog/VHDL) lower their ASTs into this
//! IR; elaboration and the kernel only ever operate on this, never on a
//! frontend's own AST.

/// Placeholder for the elaborated design graph. Real shape TBD in phase 1:
/// module hierarchy, nets/signals, and comb/seq process nodes forming a
/// dataflow graph the kernel can topologically schedule.
pub struct Design;
