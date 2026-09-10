//! Common intermediate representation shared by all language frontends.
//!
//! Frontends (Verilog, later SystemVerilog/VHDL) lower their ASTs into this
//! IR; elaboration and the kernel only ever operate on this, never on a
//! frontend's own AST.
//!
//! v1 scope (deliberately narrow -- see docs/decisions.md for the
//! interpreter-first sequencing this supports): a single flat module, no
//! instances/hierarchy, no parameters/generate blocks, any number of
//! clocked (`always @(posedge clk)`) processes but no combinational
//! (`always_comb`/`assign`) processes yet, `if`/`else` (no `else if`) and
//! non-blocking assignment only, no `case`, no bit-select/concatenation.
//! Each of those is a documented gap to widen incrementally, not a final
//! design.

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
    /// Logical negation (`!`) -- result is always 0 or 1. Bitwise `~` is
    /// not supported yet: doing it correctly needs each sub-expression's
    /// width tracked so the complement gets masked at the point of
    /// negation, not just when the final result is written to a signal
    /// (see docs/decisions.md and this crate's kernel counterpart for why
    /// write-time-only masking is fine for the operators below but not for
    /// `~`).
    Not(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    /// Result is always 0 or 1, like all comparison/logical variants below.
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Le(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    Ge(Box<Expr>, Box<Expr>),
    LogicalAnd(Box<Expr>, Box<Expr>),
    LogicalOr(Box<Expr>, Box<Expr>),
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

/// A single `always @(posedge <clock>) begin ... end` block. A module can
/// have any number of these; combinational (`always_comb`/`assign`)
/// processes are not lowered yet (see this crate's doc comment).
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
