//! Common intermediate representation shared by all language frontends.
//!
//! Frontends (Verilog, later SystemVerilog/VHDL) lower their ASTs into this
//! IR; elaboration and the kernel only ever operate on this, never on a
//! frontend's own AST.
//!
//! v1 scope (deliberately narrow -- see docs/decisions.md for the
//! interpreter-first sequencing this supports): a single flat module, no
//! instances/hierarchy, no module parameters (`#(parameter ...)`) and no
//! generate blocks (so no array/memory signals either -- `reg [31:0] mem
//! [0:31]`-style declarations aren't lowered, a distinct and
//! likely-larger gap from bit-select on a single signal), any number of
//! clocked (`always @(posedge clk)`) processes and continuous `assign`s
//! but no `always_comb` yet, `if`/`else`/`else if` and
//! `case`/`casez`/`casex` (see `CaseValue`) alongside non-blocking
//! assignment, constant and variable bit-select, constant part-select,
//! concatenation, and the ternary operator on reads (no indexed
//! part-select `x[base +: width]`), plus a constant bit-select/part-select
//! as a non-blocking-assignment *target* (`x[7:0] <= v;`) -- a variable
//! index or indexed range as a target isn't supported, and neither is a
//! select as a *continuous*-assignment target (`assign x[7:0] = v;`; only
//! `<=` supports a partial write, since it alone has a commit phase to do
//! the read-modify-write in). A concatenation of such targets
//! (`{a, b[3:0]} <= v;`) needs no IR support of its own at all -- the
//! frontend splits it into several plain `Stmt::NonBlockingAssign`s, one
//! per part, at lowering time (see `ictus-frontend-verilog`'s
//! `lower_concat_target_assign`). Each of those is a documented gap to
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
    /// Bit-select (`x[3]`, `msb == lsb`) or part-select (`x[7:0]`), with a
    /// *constant* index/bounds fixed at lowering time -- for a
    /// runtime-computed index (`x[i]`), see `DynamicBitSelect` below. This
    /// is the read-side expression form; a select as a non-blocking
    /// *assignment target* (`x[3:0] <= v;`) is represented separately, as
    /// `Stmt::NonBlockingAssign`'s `target_range` field, not as an `Expr`
    /// at all -- see that field's doc comment. Unlike most other operators
    /// here, this one masks its own result immediately (to
    /// `msb - lsb + 1` bits) in the kernel rather than relying on masking
    /// happening later at signal-write time, since its width is exactly
    /// known.
    Select { base: Box<Expr>, msb: u32, lsb: u32 },
    /// Bit-select with a runtime-computed index (`x[i]`), always 1 bit
    /// wide. No indexed *part*-select (`x[base +: width]`, a fixed width
    /// at a variable base) yet -- only single-bit -- and, like `Select`,
    /// not supported as an assignment target (a target's write range must
    /// be known at lowering time, not computed per-cycle). An index that's out of
    /// range for `base`'s width (including simply `>= 64`) returns 0
    /// rather than panicking or propagating 'x' -- this kernel is 2-state
    /// only (decisions.md D6) and has no 'x' to propagate; 0 is a
    /// deliberate, documented choice, not an accident, but it does mean
    /// this won't match a 4-state reference simulator's output for an
    /// out-of-range index (see `ictus_kernel`'s test for this rather than
    /// a differential one, for exactly that reason).
    DynamicBitSelect { base: Box<Expr>, index: Box<Expr> },
    /// Concatenation (`{a, b, c}`), MSB-first (`a` occupies the highest
    /// bits of the result) -- matching Verilog's own left-to-right order.
    /// Each part carries its own bit width, computed by the frontend at
    /// lowering time (see `ictus_frontend_verilog::expr_width`) rather
    /// than re-derived by the kernel on every evaluation; this also means
    /// only expression forms the frontend can determine a static width
    /// for (literals, signal references, select, nested concatenation)
    /// can appear as a concatenation operand -- arithmetic/comparison
    /// results are rejected there rather than guessed at. Like `Select`,
    /// this masks each part to its own declared width immediately during
    /// evaluation, not deferred to signal-write time.
    Concat(Vec<(Expr, u32)>),
    /// Ternary/conditional operator (`cond ? then_val : else_val`).
    Ternary {
        cond: Box<Expr>,
        then_val: Box<Expr>,
        else_val: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    NonBlockingAssign {
        target: SignalId,
        /// When `Some((msb, lsb))`, only that bit range of `target` is
        /// written (a constant bit-select/part-select target, e.g.
        /// `x[7:0] <= value;`); the rest of the signal is left unchanged.
        /// `None` means the whole signal is replaced.
        target_range: Option<(u32, u32)>,
        value: Expr,
    },
    If {
        cond: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    /// `case`, `casez`, or `casex` -- arms tried in order, first matching
    /// value wins (matching multiple comma-separated values, e.g.
    /// `2'd2, 2'd3: ...`, is one `CaseArm` with several `values`). Each
    /// value is independently exact-match or wildcard-match (see
    /// `CaseValue`) -- mixing both kinds of value within one `casez`'s
    /// arms is valid and not unusual (most arms are wildcard patterns, a
    /// `default` or a specific literal arm might not need any `?` bits).
    Case {
        selector: Expr,
        arms: Vec<CaseArm>,
        default: Vec<Stmt>,
    },
}

#[derive(Debug, Clone)]
pub struct CaseArm {
    pub values: Vec<CaseValue>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum CaseValue {
    /// Plain `case` semantics, or a non-wildcard item inside a
    /// `casez`/`casex`: matches when `expr`'s value equals the selector's.
    Exact(Expr),
    /// A `casez`/`casex` item with `?`/`z`/`x` wildcard bits (e.g.
    /// `8'b1010????`). Matches when `(selector ^ value) & care_mask == 0`
    /// -- i.e. every bit where `care_mask` is 1 must match exactly, and
    /// every bit where it's 0 matches regardless of the selector's value
    /// there. Bit positions beyond the literal's own written width are
    /// *not* covered by `care_mask` (so they're effectively wildcard too,
    /// not zero-extended the way real Verilog would treat an undersized
    /// literal) -- a known simplification, fine as long as a wildcard
    /// item's declared width matches the selector's, which is standard
    /// style for this construct and the only style this frontend's tests
    /// use.
    Wildcard { value: u64, care_mask: u64 },
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
