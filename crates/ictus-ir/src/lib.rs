//! Common intermediate representation shared by all language frontends.
//!
//! Frontends (Verilog, later SystemVerilog/VHDL) lower their ASTs into this
//! IR; elaboration and the kernel only ever operate on this, never on a
//! frontend's own AST.
//!
//! v1 scope (deliberately narrow -- see docs/decisions.md for the
//! interpreter-first sequencing this supports): a single flat module, no
//! instances/hierarchy, no parameters/generate blocks, one clocked
//! (`always @(posedge clk)`) process per module, `if`/`else` and
//! non-blocking assignment only. Each of those is a documented gap to
//! widen incrementally, not a final design.

/// A signal's index into `Module::signals`. Cheap to copy; stable for the
/// lifetime of a `Module` (signals are never removed after lowering).
pub type SignalId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub name: String,
    /// Bit width. 1 for a plain `wire`/`reg`; >1 for `[N:0]`-style vectors.
    pub width: u32,
    /// `None` for an internal signal (not a module port).
    pub direction: Option<Direction>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal { value: u64, width: u32 },
    Ref(SignalId),
    Not(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    NonBlockingAssign { target: SignalId, value: Expr },
    If {
        cond: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
}

/// A single `always @(posedge <clock>) begin ... end` block. v1 supports
/// exactly one of these per module -- multiple clocked processes and
/// combinational (`always_comb`/`assign`) processes are not lowered yet.
#[derive(Debug, Clone)]
pub struct ClockedProcess {
    pub clock: SignalId,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub name: String,
    pub signals: Vec<Signal>,
    pub clocked_processes: Vec<ClockedProcess>,
}

impl Module {
    pub fn signal_id(&self, name: &str) -> Option<SignalId> {
        self.signals.iter().position(|s| s.name == name)
    }

    pub fn push_signal(&mut self, signal: Signal) -> SignalId {
        self.signals.push(signal);
        self.signals.len() - 1
    }
}
