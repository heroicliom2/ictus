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

use ictus_ir::{Expr, Module, SignalId, Stmt};

pub struct Simulation<'m> {
    module: &'m Module,
    values: Vec<u64>,
}

impl<'m> Simulation<'m> {
    pub fn new(module: &'m Module) -> Self {
        Self {
            module,
            values: vec![0; module.signals.len()],
        }
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
        let mut updates = Vec::new();
        for process in &self.module.clocked_processes {
            eval_stmts(&process.body, &self.values, &mut updates);
        }
        for (id, value) in updates {
            self.values[id] = mask(value, self.module.signals[id].width);
        }
    }
}

fn eval_stmts(stmts: &[Stmt], values: &[u64], updates: &mut Vec<(SignalId, u64)>) {
    for stmt in stmts {
        match stmt {
            Stmt::NonBlockingAssign { target, value } => {
                updates.push((*target, eval_expr(value, values)));
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let branch = if eval_expr(cond, values) != 0 {
                    then_branch
                } else {
                    else_branch
                };
                eval_stmts(branch, values, updates);
            }
        }
    }
}

fn eval_expr(expr: &Expr, values: &[u64]) -> u64 {
    let bool_val = |b: bool| u64::from(b);
    match expr {
        Expr::Literal { value, .. } => *value,
        Expr::Ref(id) => values[*id],
        // Verilog `!` is logical negation (result is 0 or 1), not a
        // bitwise complement across the operand's width -- that's `~`,
        // which this frontend doesn't lower yet.
        Expr::Not(inner) => bool_val(eval_expr(inner, values) == 0),
        Expr::Add(lhs, rhs) => eval_expr(lhs, values).wrapping_add(eval_expr(rhs, values)),
        Expr::And(lhs, rhs) => eval_expr(lhs, values) & eval_expr(rhs, values),
        Expr::Or(lhs, rhs) => eval_expr(lhs, values) | eval_expr(rhs, values),
        Expr::Xor(lhs, rhs) => eval_expr(lhs, values) ^ eval_expr(rhs, values),
        Expr::Eq(lhs, rhs) => bool_val(eval_expr(lhs, values) == eval_expr(rhs, values)),
        Expr::Ne(lhs, rhs) => bool_val(eval_expr(lhs, values) != eval_expr(rhs, values)),
        Expr::Lt(lhs, rhs) => bool_val(eval_expr(lhs, values) < eval_expr(rhs, values)),
        Expr::Le(lhs, rhs) => bool_val(eval_expr(lhs, values) <= eval_expr(rhs, values)),
        Expr::Gt(lhs, rhs) => bool_val(eval_expr(lhs, values) > eval_expr(rhs, values)),
        Expr::Ge(lhs, rhs) => bool_val(eval_expr(lhs, values) >= eval_expr(rhs, values)),
        Expr::LogicalAnd(lhs, rhs) => {
            bool_val(eval_expr(lhs, values) != 0 && eval_expr(rhs, values) != 0)
        }
        Expr::LogicalOr(lhs, rhs) => {
            bool_val(eval_expr(lhs, values) != 0 || eval_expr(rhs, values) != 0)
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
        });
        let resetn = m.push_signal(Signal {
            name: "resetn".to_string(),
            width: 1,
            direction: Some(Direction::Input),
        });
        let count = m.push_signal(Signal {
            name: "count".to_string(),
            width: 8,
            direction: Some(Direction::Output),
        });

        m.clocked_processes.push(ClockedProcess {
            clock: clk,
            body: vec![Stmt::If {
                cond: Expr::Not(Box::new(Expr::Ref(resetn))),
                then_branch: vec![Stmt::NonBlockingAssign {
                    target: count,
                    value: Expr::Literal { value: 0, width: 8 },
                }],
                else_branch: vec![Stmt::NonBlockingAssign {
                    target: count,
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
}
