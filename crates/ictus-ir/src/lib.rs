//! Common intermediate representation shared by all language frontends.
//!
//! Frontends (Verilog, later SystemVerilog/VHDL) lower their ASTs into this
//! IR; elaboration and the kernel only ever operate on this, never on a
//! frontend's own AST.
//!
//! v1 scope (deliberately narrow -- see docs/decisions.md for the
//! interpreter-first sequencing this supports): a single flat module, no
//! instances/hierarchy, no parameters/generate blocks, any number of
//! clocked (`always @(posedge clk)`) processes and continuous `assign`s
//! but no `always_comb` yet, `if`/`else` (no `else if`) and plain `case`
//! (exact match; not `casez`/`casex`, which need wildcard-bit-aware
//! comparison this IR doesn't represent yet) alongside non-blocking
//! assignment, no bit-select/concatenation. Each of those is a documented
//! gap to widen incrementally, not a final design.

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
    /// Bit-select (`x[3]`, `msb == lsb`) or part-select (`x[7:0]`).
    /// `msb`/`lsb` are constants fixed at lowering time -- v1 doesn't
    /// support a variable/signal-indexed select (`x[i]`), and doesn't
    /// support a select as an *assignment target* (`x[3:0] <= v;`) either;
    /// the frontend rejects both rather than silently lowering them as a
    /// full-width reference/write. Unlike most other operators here, this
    /// one masks its own result immediately (to `msb - lsb + 1` bits) in
    /// the kernel rather than relying on masking happening later at
    /// signal-write time, since its width is exactly known.
    Select { base: Box<Expr>, msb: u32, lsb: u32 },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    NonBlockingAssign { target: SignalId, value: Expr },
    If {
        cond: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    /// Plain `case` only -- exact equality against `selector`, arms tried
    /// in order, first match wins (matching multiple comma-separated
    /// values, e.g. `2'd2, 2'd3: ...`, is one `CaseArm` with several
    /// `values`). `casez`/`casex` are rejected by the frontend rather
    /// than silently treated as exact-match `case`, which would silently
    /// mis-match on their wildcard bits.
    Case {
        selector: Expr,
        arms: Vec<CaseArm>,
        default: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub struct CaseArm {
    pub values: Vec<Expr>,
    pub body: Vec<Stmt>,
}

/// A single `always @(posedge <clock>) begin ... end` block. A module can
/// have any number of these; `always_comb` is not lowered yet (see this
/// crate's doc comment) -- use `Assign` (below) for combinational logic.
#[derive(Debug, Clone)]
pub struct ClockedProcess {
    pub clock: SignalId,
    pub body: Vec<Stmt>,
}

/// A continuous assignment (`assign target = value;`). Unlike
/// `Stmt::NonBlockingAssign`, this isn't triggered by a clock edge --
/// conceptually it's always active, so the kernel re-evaluates every
/// `Assign` in a module whenever it needs combinational logic to be
/// current (see `ictus_kernel::Simulation`'s doc comment for exactly
/// when). No conditional form (`if`) exists at this level: a `?:`
/// ternary inside `value` would be the Verilog-faithful way to express
/// conditional combinational logic, but that operator isn't lowered yet.
#[derive(Debug, Clone)]
pub struct Assign {
    pub target: SignalId,
    pub value: Expr,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub name: String,
    pub signals: Vec<Signal>,
    pub clocked_processes: Vec<ClockedProcess>,
    pub assigns: Vec<Assign>,
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
