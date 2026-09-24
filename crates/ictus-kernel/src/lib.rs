//! Execution engine.
//!
//! This is deliberately an *interpreter*, not the Cranelift-JIT compiled
//! engine docs/architecture.md describes as the eventual kernel. Building
//! JIT codegen before there's a correctness baseline to check it against
//! would mean optimizing before proving anything works -- see
//! docs/decisions.md for this sequencing choice. This interpreter (and the
//! differential tests built on it) *is* that baseline; Cranelift-based
//! codegen replaces its internals later without changing `Simulation`'s
//! external behavior.
//!
//! Semantics implemented: cycle-based (docs/architecture.md, "Kernel") --
//! `tick()` evaluates one rising edge of every clocked process. Per
//! process, every non-blocking assignment's right-hand side is evaluated
//! against the state as it was *before* this tick, and only then are all
//! the resulting writes committed together -- matching Verilog's
//! non-blocking assignment rule that `<=` reads pre-edge values regardless
//! of statement order within the same clock edge.
//!
//! A *blocking* assignment (`=`) is the other half of that rule, and the
//! one exception to "nothing is written during evaluation": it writes
//! immediately, so the statements after it read the new value, which is
//! what makes it usable as a local variable mid-sequence. The two
//! disciplines run side by side in one pass -- `eval_stmts` mutates state
//! for a blocking write and queues a non-blocking one, and the queue
//! drains afterwards -- which is Verilog's own active-region/NBA-region
//! ordering rather than an approximation of it. Both go through the same
//! `apply_write`, so the two differ in *when* that function is called and
//! in nothing else (see `ictus_ir::Stmt::BlockingAssign`, decisions.md
//! D23). A write with a
//! `target_range` (`x[7:0] <= v;` -- a constant bit-select/part-select
//! target, see `ictus_ir::Stmt::NonBlockingAssign`) is committed as a
//! read-modify-write against the signal's current stored value rather
//! than a full-width replace, so it leaves the rest of the signal's bits
//! untouched; see the commit loop in `tick()` for why applying these in
//! declaration order via direct mutation (rather than staging them in a
//! separate map first) is still correct even when more than one partial
//! write targets the same signal in one tick.
//!
//! Combinational logic (`ictus_ir::Assign`, i.e. `assign target = value;`)
//! is "settled" -- every `Assign` re-evaluated and written immediately,
//! not deferred like non-blocking assignment -- at two points per `tick()`:
//! once *before* clocked processes run (so a clocked process reading a
//! combinationally-derived signal sees it reflect the pre-edge register
//! state, matching real continuous-assignment semantics) and once *after*
//! they commit (so a caller reading state right after `tick()` -- or the
//! *next* `tick()`'s initial settle -- sees combinational logic reflect
//! the registers this edge just updated). `new()` also settles once, so a
//! `Simulation` that's never been ticked still has correct combinational
//! output for its (zeroed) initial state.
//!
//! Settling is a **single pass over `Module::assigns` in declaration
//! order**, not a full fixed-point/topological solve. This is correct as
//! long as combinational signals are declared in dependency order in the
//! source (the overwhelmingly common style, and the only style this
//! frontend's test fixtures use) -- a signal assigned from another
//! combinational signal declared *later* in the source won't see that
//! later signal's fresh value until the *next* settle call. Worth fixing
//! properly (topological sort, matching the kernel's eventual compiled
//! design in docs/architecture.md) if this ever bites a real design;
//! flagged here rather than silently trusted.

use ictus_ir::{Expr, Module, SignalId, Stmt};

pub struct Simulation<'m> {
    module: &'m Module,
    values: Vec<u64>,
    /// Array signals' contents, indexed by the same `SignalId` as
    /// `values` -- an empty `Vec` for every scalar signal, so the index
    /// spaces line up and a lookup stays O(1) without a map. Kept as a
    /// side table rather than packed into `values` deliberately: it leaves
    /// scalar access (by far the common case) exactly as it was, and this
    /// interpreter's storage layout isn't the one that has to be fast --
    /// see this module's opening comment on Cranelift replacing these
    /// internals, and docs/architecture.md on the bit-packed contiguous
    /// layout the compiled kernel is meant to end up with instead.
    arrays: Vec<Vec<u64>>,
}

impl<'m> Simulation<'m> {
    pub fn new(module: &'m Module) -> Self {
        let arrays = module
            .signals
            .iter()
            .map(|signal| match signal.depth {
                Some(depth) => vec![0; depth as usize],
                None => Vec::new(),
            })
            .collect();
        let mut sim = Self {
            module,
            values: vec![0; module.signals.len()],
            arrays,
        };
        sim.settle_combinational();
        sim
    }

    /// Reads one element of an array signal by name, for a test or driver
    /// that needs to look inside one (`sim.get`/`set` address scalars, and
    /// an array has no single value for them to return).
    pub fn get_array(&self, name: &str, index: usize) -> u64 {
        let id = self
            .module
            .signal_id(name)
            .unwrap_or_else(|| panic!("unknown signal '{name}'"));
        *self.arrays[id]
            .get(index)
            .unwrap_or_else(|| panic!("index {index} is out of range for array '{name}'"))
    }

    pub fn set(&mut self, name: &str, value: u64) {
        let id = self
            .module
            .signal_id(name)
            .unwrap_or_else(|| panic!("unknown signal '{name}'"));
        self.values[id] = mask(value, self.module.signals[id].width);
    }

    pub fn get(&self, name: &str) -> u64 {
        let id = self
            .module
            .signal_id(name)
            .unwrap_or_else(|| panic!("unknown signal '{name}'"));
        self.values[id]
    }

    /// Evaluates one rising edge on every clocked process in the module.
    pub fn tick(&mut self) {
        self.settle_combinational();

        // `module` is copied out of `self` first so the loop below can
        // borrow `self.values`/`self.arrays` mutably: a *blocking*
        // assignment inside a process writes them as it goes, so
        // evaluation can't hold `self` immutably the way it did when
        // every write was deferred.
        let module = self.module;
        let mut updates = Vec::new();
        for process in &module.clocked_processes {
            eval_stmts(
                &process.body,
                module,
                &mut self.values,
                &mut self.arrays,
                &mut updates,
            );
        }
        // Applied in order via direct mutation, not staged in a separate
        // map: every *non-blocking* RHS above was already evaluated before
        // this loop starts, so a later partial write to the same signal
        // correctly builds on an earlier one from this same tick (matching
        // real hardware, where multiple non-blocking writes to the same
        // bits in one process is "last write wins" and writes to disjoint
        // bit ranges combine).
        //
        // This is Verilog's own two-region ordering: blocking assignments
        // take effect during statement execution (above), queued
        // non-blocking ones all land afterwards (here).
        for update in updates {
            apply_write(module, &mut self.values, &mut self.arrays, update);
        }

        self.settle_combinational();
    }

    /// Re-evaluates every continuous `assign` once, in declaration order.
    /// See this module's doc comment for when `tick()` calls this and why.
    fn settle_combinational(&mut self) {
        for assign in &self.module.assigns {
            let value = eval_expr(&assign.value, &self.values, &self.arrays);
            self.values[assign.target] = mask(value, self.module.signals[assign.target].width);
        }
    }
}

/// One write collected during a tick's evaluation phase, to be applied
/// once every right-hand side (and every array index) has been evaluated
/// against the pre-edge snapshot -- see `tick`'s commit loop.
enum PendingWrite {
    Scalar {
        target: SignalId,
        range: Option<(u32, u32)>,
        value: u64,
    },
    /// The index is stored already-evaluated, not as an expression: it has
    /// to be read from the same pre-edge snapshot as `value`, so
    /// re-evaluating it at commit time (after earlier writes in this same
    /// tick have landed) would be wrong.
    Array {
        array: SignalId,
        index: u64,
        value: u64,
    },
}

/// Performs one write against live state. Shared by both assignment
/// kinds: a blocking assignment calls this the moment it's evaluated, a
/// non-blocking one has it called from `tick`'s commit loop once every
/// right-hand side has been read. The two differ in *when* this runs,
/// never in what it does -- which is why the two `Stmt` variants for them
/// duplicate only field lists, not behavior (see
/// `ictus_ir::Stmt::BlockingAssign`).
fn apply_write(
    module: &Module,
    values: &mut [u64],
    arrays: &mut [Vec<u64>],
    write: PendingWrite,
) {
    match write {
        PendingWrite::Scalar { target, range: None, value } => {
            values[target] = mask(value, module.signals[target].width)
        }
        PendingWrite::Scalar { target, range: Some((msb, lsb)), value } => {
            let width = msb - lsb + 1;
            let clear_mask = mask(u64::MAX, width) << lsb;
            let current = values[target] & !clear_mask;
            let new_bits = (mask(value, width)) << lsb;
            values[target] = mask(current | new_bits, module.signals[target].width);
        }
        // An index past the end of the array drops the write -- there's no
        // element to update, and clamping or wrapping it would corrupt a
        // *different* element, which is worse than doing nothing. See
        // `ictus_ir::Stmt::ArrayAssign`.
        PendingWrite::Array { array, index, value } => {
            let width = module.signals[array].width;
            if let Some(slot) = usize::try_from(index)
                .ok()
                .and_then(|index| arrays[array].get_mut(index))
            {
                *slot = mask(value, width);
            }
        }
    }
}

fn eval_stmts(
    stmts: &[Stmt],
    module: &Module,
    values: &mut [u64],
    arrays: &mut [Vec<u64>],
    updates: &mut Vec<PendingWrite>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::NonBlockingAssign {
                target,
                target_range,
                value,
            } => {
                updates.push(PendingWrite::Scalar {
                    target: *target,
                    range: *target_range,
                    value: eval_expr(value, values, arrays),
                });
            }
            Stmt::ArrayAssign {
                array,
                index,
                value,
            } => {
                updates.push(PendingWrite::Array {
                    array: *array,
                    index: eval_expr(index, values, arrays),
                    value: eval_expr(value, values, arrays),
                });
            }
            // Blocking: evaluated and written right here, so the next
            // statement -- and any later `<=`'s right-hand side -- sees
            // the new value.
            Stmt::BlockingAssign {
                target,
                target_range,
                value,
            } => {
                let write = PendingWrite::Scalar {
                    target: *target,
                    range: *target_range,
                    value: eval_expr(value, values, arrays),
                };
                apply_write(module, values, arrays, write);
            }
            Stmt::BlockingArrayAssign {
                array,
                index,
                value,
            } => {
                let write = PendingWrite::Array {
                    array: *array,
                    index: eval_expr(index, values, arrays),
                    value: eval_expr(value, values, arrays),
                };
                apply_write(module, values, arrays, write);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let branch = if eval_expr(cond, values, arrays) != 0 {
                    then_branch
                } else {
                    else_branch
                };
                eval_stmts(branch, module, values, arrays, updates);
            }
            Stmt::Case {
                selector,
                arms,
                default,
            } => {
                let selector_value = eval_expr(selector, values, arrays);
                let matched_arm = arms.iter().find(|arm| {
                    arm.values
                        .iter()
                        .any(|v| case_value_matches(v, selector_value, values, arrays))
                });
                match matched_arm {
                    Some(arm) => eval_stmts(&arm.body, module, values, arrays, updates),
                    None => eval_stmts(default, module, values, arrays, updates),
                }
            }
        }
    }
}

fn case_value_matches(
    value: &ictus_ir::CaseValue,
    selector_value: u64,
    values: &[u64],
    arrays: &[Vec<u64>],
) -> bool {
    match value {
        ictus_ir::CaseValue::Exact(expr) => eval_expr(expr, values, arrays) == selector_value,
        ictus_ir::CaseValue::Wildcard { value, care_mask } => {
            (selector_value ^ value) & care_mask == 0
        }
    }
}

fn eval_expr(expr: &Expr, values: &[u64], arrays: &[Vec<u64>]) -> u64 {
    let bool_val = |b: bool| u64::from(b);
    match expr {
        Expr::Literal { value, .. } => *value,
        Expr::Ref(id) => values[*id],
        // An index past the end reads 0 -- see `ictus_ir::Expr::ArrayRead`
        // for why that's the deliberate choice in a 2-state kernel rather
        // than an oversight.
        Expr::ArrayRead { array, index } => {
            let index = eval_expr(index, values, arrays);
            usize::try_from(index)
                .ok()
                .and_then(|index| arrays[*array].get(index))
                .copied()
                .unwrap_or(0)
        }
        // Verilog `!` is logical negation (result is 0 or 1), not a
        // bitwise complement across the operand's width -- that's `~`,
        // handled by `BitwiseNot` below.
        Expr::Not(inner) => bool_val(eval_expr(inner, values, arrays) == 0),
        Expr::BitwiseNot(inner, width) => mask(!eval_expr(inner, values, arrays), *width),
        Expr::ReduceAnd(inner, width) => {
            let value = mask(eval_expr(inner, values, arrays), *width);
            bool_val(value == mask(u64::MAX, *width))
        }
        Expr::ReduceOr(inner, width) => bool_val(mask(eval_expr(inner, values, arrays), *width) != 0),
        Expr::ReduceXor(inner, width) => {
            let value = mask(eval_expr(inner, values, arrays), *width);
            bool_val(value.count_ones() % 2 == 1)
        }
        Expr::Add(lhs, rhs) => eval_expr(lhs, values, arrays).wrapping_add(eval_expr(rhs, values, arrays)),
        Expr::Sub(lhs, rhs) => eval_expr(lhs, values, arrays).wrapping_sub(eval_expr(rhs, values, arrays)),
        Expr::Mul(lhs, rhs) => eval_expr(lhs, values, arrays).wrapping_mul(eval_expr(rhs, values, arrays)),
        // A shift amount of 64 or more shifts every bit out (0), rather
        // than panicking the way a bare `<<`/`>>` on a u64 would or
        // silently taking the amount modulo 64 the way `wrapping_shl`
        // would -- neither of which is what Verilog means.
        Expr::Shl(lhs, rhs) => {
            let value = eval_expr(lhs, values, arrays);
            match u32::try_from(eval_expr(rhs, values, arrays)) {
                Ok(shift) => value.checked_shl(shift).unwrap_or(0),
                Err(_) => 0,
            }
        }
        Expr::Shr(lhs, rhs) => {
            let value = eval_expr(lhs, values, arrays);
            match u32::try_from(eval_expr(rhs, values, arrays)) {
                Ok(shift) => value.checked_shr(shift).unwrap_or(0),
                Err(_) => 0,
            }
        }
        // Correct as a plain `i64` shift only because the frontend only
        // ever builds this with an already-sign-extended `Signed` left
        // operand -- see `ictus_ir::Expr::AShr`'s doc comment. Clamped to
        // 63 rather than guarded at 0: shifting an `i64` right by 63
        // already replicates the sign bit across every bit, so any larger
        // amount means the same thing.
        Expr::AShr(lhs, rhs) => {
            let value = eval_expr(lhs, values, arrays) as i64;
            let shift = u32::try_from(eval_expr(rhs, values, arrays)).unwrap_or(u32::MAX).min(63);
            (value >> shift) as u64
        }
        Expr::And(lhs, rhs) => eval_expr(lhs, values, arrays) & eval_expr(rhs, values, arrays),
        Expr::Or(lhs, rhs) => eval_expr(lhs, values, arrays) | eval_expr(rhs, values, arrays),
        Expr::Xor(lhs, rhs) => eval_expr(lhs, values, arrays) ^ eval_expr(rhs, values, arrays),
        Expr::Eq(lhs, rhs) => bool_val(eval_expr(lhs, values, arrays) == eval_expr(rhs, values, arrays)),
        Expr::Ne(lhs, rhs) => bool_val(eval_expr(lhs, values, arrays) != eval_expr(rhs, values, arrays)),
        Expr::Lt(lhs, rhs) => bool_val(eval_expr(lhs, values, arrays) < eval_expr(rhs, values, arrays)),
        Expr::Le(lhs, rhs) => bool_val(eval_expr(lhs, values, arrays) <= eval_expr(rhs, values, arrays)),
        Expr::Gt(lhs, rhs) => bool_val(eval_expr(lhs, values, arrays) > eval_expr(rhs, values, arrays)),
        Expr::Ge(lhs, rhs) => bool_val(eval_expr(lhs, values, arrays) >= eval_expr(rhs, values, arrays)),
        // Correct as a plain `i64` comparison only because the frontend
        // only ever builds this with two already-sign-extended `Signed`
        // operands -- see `ictus_ir::Expr::SignedLt`'s doc comment.
        Expr::SignedLt(lhs, rhs) => {
            bool_val((eval_expr(lhs, values, arrays) as i64) < (eval_expr(rhs, values, arrays) as i64))
        }
        Expr::LogicalAnd(lhs, rhs) => {
            bool_val(eval_expr(lhs, values, arrays) != 0 && eval_expr(rhs, values, arrays) != 0)
        }
        Expr::LogicalOr(lhs, rhs) => {
            bool_val(eval_expr(lhs, values, arrays) != 0 || eval_expr(rhs, values, arrays) != 0)
        }
        Expr::Select { base, msb, lsb } => {
            mask(eval_expr(base, values, arrays) >> lsb, msb - lsb + 1)
        }
        Expr::Concat(parts) => {
            let mut result = 0u64;
            for (part, width) in parts {
                result = (result << width) | mask(eval_expr(part, values, arrays), *width);
            }
            result
        }
        Expr::DynamicBitSelect { base, index } => {
            let index = eval_expr(index, values, arrays);
            // A `>= 64` shift on a u64 panics (it's undefined behavior
            // for the underlying shift instruction) -- and any index
            // beyond base's actual width is out of range regardless, so
            // this also covers "in range for u64 but not for the real
            // signal" the same way: 0, not a panic, not a guess.
            if index >= 64 {
                0
            } else {
                (eval_expr(base, values, arrays) >> index) & 1
            }
        }
        Expr::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            if eval_expr(cond, values, arrays) != 0 {
                eval_expr(then_val, values, arrays)
            } else {
                eval_expr(else_val, values, arrays)
            }
        }
        Expr::Signed(inner, width) => {
            let width = *width;
            let value = eval_expr(inner, values, arrays);
            // `width >= 64` (or, defensively, `== 0`) has no room to
            // extend into -- and shifting a u64 by 64 or more is
            // undefined behavior for the underlying shift instruction --
            // so the value is already whatever it is, full stop.
            if width == 0 || width >= 64 {
                value
            } else {
                let value = mask(value, width);
                let sign_bit_set = (value >> (width - 1)) & 1 == 1;
                if sign_bit_set {
                    // Replicate the sign bit into every bit above
                    // `width` -- real two's-complement sign extension.
                    // Everything downstream (Select's own masking, or
                    // the kernel's commit-time write mask) then keeps
                    // only however many of these bits its own narrower
                    // context actually needs.
                    value | (u64::MAX << width)
                } else {
                    value
                }
            }
        }
    }
}

fn mask(value: u64, width: u32) -> u64 {
    if width >= 64 {
        value
    } else {
        value & ((1u64 << width) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ictus_ir::{ClockedProcess, Direction, Signal};

    /// Hand-built IR for the same `counter` module
    /// `ictus-frontend-verilog/tests/fixtures/counter.v` lowers to -- kept
    /// independent of the frontend deliberately, so this crate's own
    /// interpreter semantics are tested in isolation from parsing/lowering.
    fn counter_module() -> Module {
        let mut m = Module {
            name: "counter".to_string(),
            ..Default::default()
        };
        let clk = m.push_signal(Signal {
            name: "clk".to_string(),
            width: 1,
            direction: Some(Direction::Input),
            depth: None,
        });
        let resetn = m.push_signal(Signal {
            name: "resetn".to_string(),
            width: 1,
            direction: Some(Direction::Input),
            depth: None,
        });
        let count = m.push_signal(Signal {
            name: "count".to_string(),
            width: 8,
            direction: Some(Direction::Output),
            depth: None,
        });

        m.clocked_processes.push(ClockedProcess {
            clock: clk,
            body: vec![Stmt::If {
                cond: Expr::Not(Box::new(Expr::Ref(resetn))),
                then_branch: vec![Stmt::NonBlockingAssign {
                    target: count,
                    target_range: None,
                    value: Expr::Literal { value: 0, width: 8 },
                }],
                else_branch: vec![Stmt::NonBlockingAssign {
                    target: count,
                    target_range: None,
                    value: Expr::Add(
                        Box::new(Expr::Ref(count)),
                        Box::new(Expr::Literal { value: 1, width: 8 }),
                    ),
                }],
            }],
        });
        m
    }

    #[test]
    fn counts_up_after_reset_deasserts() {
        let module = counter_module();
        let mut sim = Simulation::new(&module);

        sim.set("resetn", 0);
        sim.tick();
        assert_eq!(sim.get("count"), 0);

        sim.set("resetn", 1);
        for expected in 1..=5u64 {
            sim.tick();
            assert_eq!(sim.get("count"), expected);
        }
    }

    #[test]
    fn count_wraps_at_8_bit_width() {
        let module = counter_module();
        let mut sim = Simulation::new(&module);

        sim.set("resetn", 1);
        for _ in 0..255 {
            sim.tick();
        }
        assert_eq!(sim.get("count"), 255);
        sim.tick();
        assert_eq!(sim.get("count"), 0, "8-bit counter must wrap 255 -> 0");
    }

    #[test]
    fn holds_reset_value_while_resetn_is_low() {
        let module = counter_module();
        let mut sim = Simulation::new(&module);

        sim.set("resetn", 1);
        sim.tick();
        sim.tick();
        assert_eq!(sim.get("count"), 2);

        sim.set("resetn", 0);
        sim.tick();
        assert_eq!(sim.get("count"), 0);
    }

    /// An out-of-range array index reads 0 and drops a write, rather than
    /// panicking, wrapping, or clamping onto a *different* element. Both
    /// are the deliberate 2-state choices documented on
    /// `ictus_ir::Expr::ArrayRead`/`Stmt::ArrayAssign`, and both are
    /// covered here rather than differentially: a 4-state reference
    /// simulator answers an out-of-range read with 'x', which has no
    /// comparable value in this kernel at all.
    #[test]
    fn out_of_range_array_access_reads_zero_and_drops_the_write() {
        let mut m = Module {
            name: "mem_module".to_string(),
            ..Default::default()
        };
        let clk = m.push_signal(Signal {
            name: "clk".to_string(),
            width: 1,
            direction: Some(Direction::Input),
            depth: None,
        });
        let mem = m.push_signal(Signal {
            name: "mem".to_string(),
            width: 8,
            direction: None,
            depth: Some(2),
        });
        let out = m.push_signal(Signal {
            name: "out".to_string(),
            width: 8,
            direction: Some(Direction::Output),
            depth: None,
        });

        // mem[5] <= 0xAB;  (out of range -- dropped)
        // out   <= mem[5];  (out of range -- reads 0)
        m.clocked_processes.push(ClockedProcess {
            clock: clk,
            body: vec![
                Stmt::ArrayAssign {
                    array: mem,
                    index: Expr::Literal { value: 5, width: 8 },
                    value: Expr::Literal { value: 0xAB, width: 8 },
                },
                Stmt::NonBlockingAssign {
                    target: out,
                    target_range: None,
                    value: Expr::ArrayRead {
                        array: mem,
                        index: Box::new(Expr::Literal { value: 5, width: 8 }),
                    },
                },
            ],
        });

        let mut sim = Simulation::new(&m);
        sim.tick();
        assert_eq!(sim.get("out"), 0, "an out-of-range read yields 0");
        // The in-range elements must be untouched -- a dropped write is
        // the point, not a write that landed somewhere else.
        assert_eq!(sim.get_array("mem", 0), 0);
        assert_eq!(sim.get_array("mem", 1), 0);
    }

    /// An array write takes effect at the commit phase like any other
    /// non-blocking assignment, so a read in the *same* tick sees the
    /// pre-edge contents -- and the index is read from that same
    /// snapshot, not re-evaluated after earlier writes have landed.
    #[test]
    fn array_write_is_visible_only_after_the_edge() {
        let mut m = Module {
            name: "mem_module".to_string(),
            ..Default::default()
        };
        let clk = m.push_signal(Signal {
            name: "clk".to_string(),
            width: 1,
            direction: Some(Direction::Input),
            depth: None,
        });
        let mem = m.push_signal(Signal {
            name: "mem".to_string(),
            width: 8,
            direction: None,
            depth: Some(2),
        });
        let out = m.push_signal(Signal {
            name: "out".to_string(),
            width: 8,
            direction: Some(Direction::Output),
            depth: None,
        });

        // mem[0] <= 0x42;  out <= mem[0];  -- in that order, in one tick.
        m.clocked_processes.push(ClockedProcess {
            clock: clk,
            body: vec![
                Stmt::ArrayAssign {
                    array: mem,
                    index: Expr::Literal { value: 0, width: 8 },
                    value: Expr::Literal { value: 0x42, width: 8 },
                },
                Stmt::NonBlockingAssign {
                    target: out,
                    target_range: None,
                    value: Expr::ArrayRead {
                        array: mem,
                        index: Box::new(Expr::Literal { value: 0, width: 8 }),
                    },
                },
            ],
        });

        let mut sim = Simulation::new(&m);
        sim.tick();
        assert_eq!(sim.get_array("mem", 0), 0x42, "the write landed this edge");
        assert_eq!(
            sim.get("out"),
            0,
            "but the same-edge read saw the pre-edge contents"
        );

        sim.tick();
        assert_eq!(sim.get("out"), 0x42, "and sees it on the next edge");
    }

    /// `~x` masks its complement to exactly `x`'s own declared width,
    /// rather than leaving the high bits of the underlying `u64` set --
    /// the reason `BitwiseNot` carries a width at all instead of relying
    /// on masking happening later at signal-write time (see its doc
    /// comment in `ictus_ir`).
    #[test]
    fn bitwise_not_masks_to_its_own_width() {
        let expr = Expr::BitwiseNot(Box::new(Expr::Literal { value: 0b0101, width: 4 }), 4);
        assert_eq!(eval_expr(&expr, &[], &[]), 0b1010);
    }

    /// A shift amount of 64 or more is the case a differential test can't
    /// reach (a realistic design's shift-amount signal is only a few bits
    /// wide), and the one where the obvious Rust spelling is wrong: a
    /// bare `<<`/`>>` panics on shift-overflow, and `wrapping_shl` would
    /// silently take the amount modulo 64 -- turning `x << 64` into
    /// `x << 0`, i.e. `x` unchanged, when Verilog says every bit is
    /// shifted out.
    #[test]
    fn logical_shifts_past_the_word_width_produce_zero() {
        let value = Expr::Literal { value: 0xFF, width: 8 };
        for amount in [64u64, 100, u64::MAX] {
            let shift = Expr::Literal { value: amount, width: 32 };
            let shl = Expr::Shl(Box::new(value.clone()), Box::new(shift.clone()));
            let shr = Expr::Shr(Box::new(value.clone()), Box::new(shift));
            assert_eq!(eval_expr(&shl, &[], &[]), 0, "0xFF << {amount}");
            assert_eq!(eval_expr(&shr, &[], &[]), 0, "0xFF >> {amount}");
        }
    }

    /// An arithmetic right shift past the word width replicates the sign
    /// bit across the whole result rather than producing 0 -- shifting a
    /// negative value right can never reach 0 in Verilog, however far it
    /// goes.
    #[test]
    fn arithmetic_shift_past_the_word_width_replicates_the_sign_bit() {
        // 8-bit 0x80 is -128; `Signed` extends it across all 64 bits, so
        // an `i64` shift sees a genuinely negative value (see
        // ictus_ir::Expr::AShr's doc comment for why that's the only case
        // the frontend ever builds an AShr for).
        let negative = Expr::Signed(Box::new(Expr::Literal { value: 0x80, width: 8 }), 8);
        let positive = Expr::Signed(Box::new(Expr::Literal { value: 0x7F, width: 8 }), 8);
        for amount in [64u64, 100, u64::MAX] {
            let shift = Expr::Literal { value: amount, width: 32 };
            let negative_shifted =
                Expr::AShr(Box::new(negative.clone()), Box::new(shift.clone()));
            let positive_shifted = Expr::AShr(Box::new(positive.clone()), Box::new(shift));
            assert_eq!(
                mask(eval_expr(&negative_shifted, &[], &[]), 8),
                0xFF,
                "-128 >>> {amount} stays all sign bits"
            );
            assert_eq!(
                eval_expr(&positive_shifted, &[], &[]),
                0,
                "+127 >>> {amount} reaches 0"
            );
        }
    }

    #[test]
    fn reduction_operators_fold_correctly() {
        let all_ones = Expr::Literal { value: 0b1111, width: 4 };
        let has_a_zero = Expr::Literal { value: 0b1011, width: 4 };
        let all_zeros = Expr::Literal { value: 0b0000, width: 4 };
        let odd_parity = Expr::Literal { value: 0b0111, width: 4 }; // 3 ones
        let even_parity = Expr::Literal { value: 0b0101, width: 4 }; // 2 ones

        assert_eq!(eval_expr(&Expr::ReduceAnd(Box::new(all_ones.clone()), 4), &[], &[]), 1);
        assert_eq!(eval_expr(&Expr::ReduceAnd(Box::new(has_a_zero), 4), &[], &[]), 0);

        assert_eq!(eval_expr(&Expr::ReduceOr(Box::new(all_zeros.clone()), 4), &[], &[]), 0);
        assert_eq!(eval_expr(&Expr::ReduceOr(Box::new(all_ones.clone()), 4), &[], &[]), 1);

        assert_eq!(eval_expr(&Expr::ReduceXor(Box::new(odd_parity), 4), &[], &[]), 1);
        assert_eq!(eval_expr(&Expr::ReduceXor(Box::new(even_parity), 4), &[], &[]), 0);
        assert_eq!(eval_expr(&Expr::ReduceXor(Box::new(all_zeros), 4), &[], &[]), 0);

        // Reduction NAND/NOR/XNOR are BitwiseNot(ReduceX(...), 1) at the
        // frontend level -- confirm that composition evaluates correctly
        // here too, not just that the frontend builds the right tree.
        let nand = Expr::BitwiseNot(Box::new(Expr::ReduceAnd(Box::new(all_ones), 4)), 1);
        assert_eq!(eval_expr(&nand, &[], &[]), 0, "NAND of all-ones is 0");
    }

    /// Two non-blocking assignments to *disjoint* bit ranges of the same
    /// signal, in the same clocked process, must both take effect in the
    /// same tick -- a partial write's commit is a read-modify-write against
    /// the signal's *current* stored value, not a full-width replace, and
    /// this must hold even when more than one partial write to the same
    /// signal lands in one tick (see `tick()`'s doc comment above the
    /// commit loop for why applying them in order via direct mutation is
    /// correct here).
    #[test]
    fn partial_writes_to_disjoint_ranges_combine_in_one_tick() {
        let mut m = Module {
            name: "nibble_pair".to_string(),
            ..Default::default()
        };
        let clk = m.push_signal(Signal {
            name: "clk".to_string(),
            width: 1,
            direction: Some(Direction::Input),
            depth: None,
        });
        let hi_in = m.push_signal(Signal {
            name: "hi_in".to_string(),
            width: 4,
            direction: Some(Direction::Input),
            depth: None,
        });
        let acc = m.push_signal(Signal {
            name: "acc".to_string(),
            width: 8,
            direction: Some(Direction::Output),
            depth: None,
        });

        m.clocked_processes.push(ClockedProcess {
            clock: clk,
            body: vec![
                // acc[3:0] <= acc[3:0] + 1;
                Stmt::NonBlockingAssign {
                    target: acc,
                    target_range: Some((3, 0)),
                    value: Expr::Add(
                        Box::new(Expr::Select {
                            base: Box::new(Expr::Ref(acc)),
                            msb: 3,
                            lsb: 0,
                        }),
                        Box::new(Expr::Literal { value: 1, width: 4 }),
                    ),
                },
                // acc[7:4] <= hi_in;
                Stmt::NonBlockingAssign {
                    target: acc,
                    target_range: Some((7, 4)),
                    value: Expr::Ref(hi_in),
                },
            ],
        });

        let mut sim = Simulation::new(&m);
        sim.set("hi_in", 0xA);
        sim.tick();
        assert_eq!(sim.get("acc"), 0xA1, "hi nibble loaded, lo nibble 0 -> 1");

        sim.set("hi_in", 0xB);
        sim.tick();
        assert_eq!(sim.get("acc"), 0xB2, "hi nibble reloaded, lo nibble 1 -> 2");
    }

    /// A partial write's range must mask/wrap within its own width, not
    /// the whole signal's -- `acc[3:0]` at 0xF must wrap to 0x0 on
    /// increment without touching `acc[7:4]`, exactly like real hardware
    /// where the RHS is evaluated in the target range's own bit width.
    #[test]
    fn partial_write_wraps_within_its_own_width_not_the_full_signal() {
        let mut m = Module {
            name: "nibble_wrap".to_string(),
            ..Default::default()
        };
        let clk = m.push_signal(Signal {
            name: "clk".to_string(),
            width: 1,
            direction: Some(Direction::Input),
            depth: None,
        });
        let acc = m.push_signal(Signal {
            name: "acc".to_string(),
            width: 8,
            direction: Some(Direction::Output),
            depth: None,
        });

        m.clocked_processes.push(ClockedProcess {
            clock: clk,
            body: vec![Stmt::NonBlockingAssign {
                target: acc,
                target_range: Some((3, 0)),
                value: Expr::Add(
                    Box::new(Expr::Select {
                        base: Box::new(Expr::Ref(acc)),
                        msb: 3,
                        lsb: 0,
                    }),
                    Box::new(Expr::Literal { value: 1, width: 4 }),
                ),
            }],
        });

        let mut sim = Simulation::new(&m);
        sim.values[acc] = 0xF0 | 0x0F; // acc = 0xFF: high nibble 0xF, low nibble 0xF
        sim.tick();
        assert_eq!(
            sim.get("acc"),
            0xF0,
            "low nibble must wrap 0xF -> 0x0 without touching the high nibble"
        );
    }

    /// A positive value (sign bit clear) is unaffected by `$signed` --
    /// its bit pattern already reads correctly as either signed or
    /// unsigned.
    #[test]
    fn signed_leaves_a_positive_value_unchanged() {
        // 6-bit 0b011111 = 31, sign bit (bit 5) clear.
        let expr = Expr::Signed(
            Box::new(Expr::Literal { value: 0b011111, width: 6 }),
            6,
        );
        assert_eq!(eval_expr(&expr, &[], &[]), 31);
    }

    /// A negative value (sign bit set) gets its sign bit replicated
    /// upward through every bit above its own declared width -- real
    /// two's-complement sign extension, not the implicit zero-extension
    /// every other expression gets. `0b100000` is -32 in 6-bit two's
    /// complement; sign-extended into (conceptually) a wider context it
    /// must read as all-1s above bit 5, e.g. as 12 bits: 0xFE0.
    #[test]
    fn signed_replicates_the_sign_bit_for_a_negative_value() {
        let expr = Expr::Signed(
            Box::new(Expr::Literal { value: 0b100000, width: 6 }),
            6,
        );
        let extended = eval_expr(&expr, &[], &[]);
        // Masking down to exactly 12 bits (as a commit-time write to a
        // 12-bit target would) must show real sign extension, not a
        // truncated positive number.
        assert_eq!(mask(extended, 12), 0xFE0);
        // And -1 in 6 bits (all 1s) sign-extends to all 1s in 12 bits.
        let all_ones = Expr::Signed(Box::new(Expr::Literal { value: 0b111111, width: 6 }), 6);
        assert_eq!(mask(eval_expr(&all_ones, &[], &[]), 12), 0xFFF);
    }

    #[test]
    fn dynamic_bit_select_reads_correct_bit() {
        // 0b1011_0010: bit0=0, bit1=1, bit3=0, bit4=1, bit7=1.
        let base = Expr::Literal {
            value: 0b1011_0010,
            width: 8,
        };
        for (index, expected) in [(0u64, 0u64), (1, 1), (3, 0), (4, 1), (7, 1)] {
            let expr = Expr::DynamicBitSelect {
                base: Box::new(base.clone()),
                index: Box::new(Expr::Literal { value: index, width: 3 }),
            };
            assert_eq!(eval_expr(&expr, &[], &[]), expected, "bit {index} of 0b1011_0010");
        }
    }

    /// An index this far out of range has no defined answer in a 2-state
    /// kernel (see ictus_ir::Expr::DynamicBitSelect's doc comment) -- the
    /// property actually worth guaranteeing is that it returns a value at
    /// all rather than panicking, since `>> 64` on a u64 is undefined
    /// behavior in Rust and this index is deliberately chosen to be `>= 64`.
    #[test]
    fn dynamic_bit_select_out_of_range_index_does_not_panic() {
        let expr = Expr::DynamicBitSelect {
            base: Box::new(Expr::Literal { value: 0xFF, width: 8 }),
            index: Box::new(Expr::Literal { value: 100, width: 32 }),
        };
        assert_eq!(eval_expr(&expr, &[], &[]), 0);
    }
}
