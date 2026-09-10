//! Cycle-based execution engine.
//!
//! Phase 1: single-threaded, 2-state, topologically evaluates the IR's
//! dataflow graph once per relevant clock edge. Cranelift JIT codegen and
//! static multi-threaded partitioning come later (phases 1 and 2).

pub fn placeholder() {}
