//! Common intermediate representation shared by all language frontends.
//!
//! Frontends (Verilog, later SystemVerilog/VHDL) lower their ASTs into this
//! IR; elaboration and the kernel only ever operate on this, never on a
//! frontend's own AST.
//!
//! v1 scope (deliberately narrow -- see docs/decisions.md for the
//! interpreter-first sequencing this supports): a single flat module, no
//! instances/hierarchy and no generate blocks; array/memory signals
//! (`reg [31:0] mem [0:31]`) *are* supported, with one unpacked
//! dimension, read and written one element at a time at a runtime index
//! (see `Signal::depth`, `Expr::ArrayRead`, `Stmt::ArrayAssign`), any
//! number of clocked (`always @(posedge clk)`) processes
//! and continuous `assign`s but no `always_comb` yet, `if`/`else`/
//! `else if` and `case`/`casez`/`casex` (see `CaseValue`) alongside
//! non-blocking assignment, `+ - * << >> >>> & | ^` and comparison/
//! logical operators (a comparison/logical/reduction result is always exactly 1
//! bit, by Verilog's own definition -- not an approximation the way a
//! general arithmetic result's width would be, so unlike `Add`/`Sub`/
//! `Mul` these are valid concatenation operands too; see
//! `ictus-frontend-verilog`'s `expr_width`), constant and variable
//! bit-select, constant part-select,
//! concatenation (including replication/multiple concatenation,
//! `{N{a,b}}` -- also just an `Expr::Concat`, its part list physically
//! repeated `N` times by the frontend at lowering time rather than given
//! its own IR representation; see `ictus-frontend-verilog`'s
//! `lower_multiple_concatenation`), the ternary operator, and
//! `$signed(...)` (see `Expr::Signed`'s doc comment -- it sign-extends a
//! value into a wider assignment target and marks operands for the
//! signed-aware operators, `Expr::AShr` and `Expr::SignedLt`, rather than
//! being a general signed type system), logical `!`, bitwise complement
//! `~`, and the reduction operators `& | ^ ~& ~| ~^`/`^~` (see
//! `Expr::BitwiseNot`/`ReduceAnd`/`ReduceOr`/`ReduceXor`'s doc comments --
//! the NAND/NOR/XNOR forms compose `BitwiseNot` with a reduction rather
//! than getting their own variants) on reads (no indexed part-select
//! `x[base +: width]`), plus
//! a constant bit-select/part-select
//! as a non-blocking-assignment *target* (`x[7:0] <= v;`) -- a variable
//! index or indexed range as a target isn't supported, and neither is a
//! select as a *continuous*-assignment target (`assign x[7:0] = v;`; only
//! `<=` supports a partial write, since it alone has a commit phase to do
//! the read-modify-write in). A concatenation of such targets
//! (`{a, b[3:0]} <= v;`) needs no IR support of its own at all -- the
//! frontend splits it into several plain `Stmt::NonBlockingAssign`s, one
//! per part, at lowering time (see `ictus-frontend-verilog`'s
//! `lower_concat_target_assign`). Likewise, a call to a *provably-empty*
//! task (`some_task;`) needs no IR support either -- the frontend lowers
//! it as zero statements, a true no-op (see
//! `lower_task_call_statement`); a call to any other task is rejected
//! rather than silently dropped. Neither does `#(parameter ...)` or
//! `localparam`: a reference is fully resolved to a plain `Expr::Literal`
//! at lowering time (see `ictus-frontend-verilog`'s `lower_parameters`/
//! `lower_constant_expr`), so `Module` has no notion of a parameter
//! existing at all, and a default/value expression can reference an
//! earlier parameter, use the ternary operator, and use arithmetic --
//! including multiplication, which the general expression grammar
//! supports too (`Expr::Mul`), not just the constant-expression one. Each
//! of those is a documented gap to widen incrementally, not a final
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
    /// Bit width of one element. 1 for a plain `wire`/`reg`; >1 for
    /// `[N:0]`-style vectors. For an array (see `depth`) this is the width
    /// of each element, not of the array as a whole.
    pub width: u32,
    /// `None` for an internal signal (not a module port).
    pub direction: Option<Direction>,
    /// `None` for an ordinary scalar/vector signal. `Some(n)` for an
    /// array (`reg [31:0] mem [0:n-1];`, Verilog's *unpacked* dimension) --
    /// `n` elements of `width` bits each, addressed by
    /// `Expr::ArrayRead`/`Stmt::ArrayAssign` rather than read or written
    /// whole. Only a single unpacked dimension is supported; a
    /// multi-dimensional array is rejected by the frontend rather than
    /// flattened behind the scenes.
    pub depth: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// A 4-state `x`/`z` digit in the Verilog source this came from
    /// (whole-value or mixed with real digits, in any base, outside a
    /// `case`/`casez`/`casex` item's own wildcard matching) is already
    /// resolved to the bit `0` by the time it reaches this variant -- see
    /// `ictus-frontend-verilog`'s `parse_binary_literal_value`/
    /// `parse_hex_literal_value` and docs/decisions.md D19. There's no
    /// other representation for "unknown" in this 2-state kernel
    /// (decisions.md D6) to preserve here even if this crate wanted to.
    Literal { value: u64, width: u32 },
    Ref(SignalId),
    /// Logical negation (`!`) -- result is always 0 or 1.
    Not(Box<Expr>),
    /// Bitwise complement (`~x`) -- inverts *every* bit, unlike `Not`
    /// (logical `!`), which collapses the whole operand to a single 0/1.
    /// Carries the operand's own natural width (the same value
    /// `ictus_frontend_verilog::expr_width` would compute for it) because,
    /// unlike the binary bitwise operators below, complementing needs to
    /// know exactly where to stop flipping bits: `!x` on an 8-bit `x` must
    /// mask its result to 8 bits immediately, not leave the high 56 bits
    /// of the underlying `u64` set and rely on masking happening later at
    /// signal-write time the way `And`/`Or`/`Xor`/`Add` safely can (those
    /// never *introduce* a 1 bit above the operands' own width, so
    /// write-time masking alone is already correct for them; complementing
    /// unset-but-out-of-range bits would, if not masked immediately).
    BitwiseNot(Box<Expr>, u32),
    /// Reduction AND (`&x`) -- folds every bit of a (width `u32`) operand
    /// down to a single bit: 1 only if *every* bit is 1. `~&x` (reduction
    /// NAND) is this composed with `BitwiseNot` at lowering time (see
    /// `ictus-frontend-verilog::lower_expr`'s `E::Unary` arm), not a
    /// separate variant -- there's nothing it needs beyond that
    /// composition.
    ReduceAnd(Box<Expr>, u32),
    /// Reduction OR (`|x`) -- 1 if *any* bit of a (width `u32`) operand is
    /// set. `~|x` (reduction NOR) composes with `BitwiseNot`, the same way
    /// `~&x` composes with `ReduceAnd` above.
    ReduceOr(Box<Expr>, u32),
    /// Reduction XOR (`^x`, parity) -- 1 if an *odd* number of bits in a
    /// (width `u32`) operand are 1. `~^x`/`^~x` (reduction XNOR) composes
    /// with `BitwiseNot`, the same way `~&x` composes with `ReduceAnd`
    /// above.
    ReduceXor(Box<Expr>, u32),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    /// Left shift (`a << b`, and `a <<< b` -- Verilog's arithmetic left
    /// shift is bit-for-bit identical to the logical one, since shifting
    /// *left* has no sign behavior to differ about). Like `Add`, the
    /// result isn't masked here: bits shifted up past the eventual
    /// target's width are dropped later, at signal-write time, which
    /// matches Verilog's own context-determined sizing for the
    /// assignment forms this lowers. A shift amount of 64 or more
    /// produces 0 (every bit shifted out), rather than Rust's
    /// shift-overflow panic or a `wrapping_shl`-style modulo-64 shift
    /// amount, neither of which is what Verilog means.
    Shl(Box<Expr>, Box<Expr>),
    /// Logical right shift (`a >> b`) -- always zero-fills, for a signed
    /// *or* unsigned operand; that's exactly why Verilog has a separate
    /// `>>>`, represented by `AShr` below. A shift amount of 64 or more
    /// produces 0, same reasoning as `Shl`.
    Shr(Box<Expr>, Box<Expr>),
    /// Arithmetic (sign-replicating) right shift (`a >>> b`) -- only ever
    /// built when the left operand is a `Signed` value, since `>>>` on an
    /// *unsigned* operand is defined by Verilog to be an ordinary logical
    /// shift and the frontend lowers that case to `Shr` instead (see
    /// `ictus-frontend-verilog::apply_binary_op`). That restriction is
    /// what makes evaluating this as a plain `i64` shift correct: a
    /// `Signed` operand has already been sign-extended across the full 64
    /// bits (see `Signed`'s own doc comment), so the sign bit an `i64`
    /// shift replicates is the operand's real sign bit, not whatever
    /// happened to land in bit 63. A shift amount of 64 or more
    /// replicates the sign bit across the whole result (all-ones for a
    /// negative value, 0 otherwise).
    AShr(Box<Expr>, Box<Expr>),
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
    /// Signed less-than (`$signed(a) < $signed(b)`) -- the *only* signed
    /// ordering comparison with its own variant. The other three are
    /// composed from it at lowering time, since each is exactly this one
    /// with its operands swapped and/or logically negated
    /// (`a > b` is `b < a`; `a <= b` is `!(b < a)`; `a >= b` is
    /// `!(a < b)`) -- identities that hold exactly for integers, the same
    /// "compose rather than add near-identical variants" approach
    /// `ReduceAnd`/`~&x` and `Shr`/unsigned-`>>>` already take. See
    /// `ictus-frontend-verilog::apply_binary_op`.
    ///
    /// Only ever built when *both* operands are `Signed`, which is what
    /// makes evaluating it as a plain `i64` comparison correct rather
    /// than accidentally correct: each operand has then already been
    /// sign-extended across all 64 bits (see `Signed`), so the sign an
    /// `i64` comparison reads is the operand's real one. Verilog's own
    /// rule that a *mixed* signed/unsigned comparison is performed
    /// unsigned isn't implemented -- it would need the signed operand
    /// re-truncated to its own width first, and no real design has needed
    /// it -- so the frontend rejects that combination instead of guessing.
    SignedLt(Box<Expr>, Box<Expr>),
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
    /// Reads one element of an array signal (`mem[i]`, where `mem` was
    /// declared with an unpacked dimension -- see `Signal::depth`). The
    /// index is evaluated at simulation time and may be any expression;
    /// a *constant* index isn't a separate case, since unlike a
    /// bit-select there's nothing to fold it into.
    ///
    /// An index outside the array's depth reads 0, the same
    /// deliberate-and-documented choice `DynamicBitSelect` makes for an
    /// out-of-range bit index and for the same reason: this kernel is
    /// 2-state (decisions.md D6) and has no 'x' to return instead. Like
    /// that case, it means an out-of-range read won't match a 4-state
    /// reference simulator, so it's covered by `ictus_kernel`'s own unit
    /// tests rather than differentially.
    ArrayRead { array: SignalId, index: Box<Expr> },
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
    /// Verilog's `$signed(inner)` -- marks `inner` (whose own natural
    /// width, the same value `ictus_frontend_verilog::expr_width` would
    /// compute for it, is carried here as the second field since a bare
    /// `Expr` doesn't otherwise know its own width) as a *signed* value.
    /// This is a no-op on `inner`'s own bits -- `$signed` doesn't resize
    /// anything itself -- but it changes what happens when the value is
    /// later used somewhere *wider* than `width`: instead of the implicit
    /// zero-extension every other expression gets when written to a wider
    /// target, the kernel replicates `inner`'s own most-significant bit
    /// (its sign bit) up through the extra bits, i.e. real two's-complement
    /// sign extension. See `ictus_kernel::eval_expr`'s `Signed` arm for
    /// the actual bit manipulation, and this field's frontend counterpart
    /// (`ictus_frontend_verilog::lower_primary`'s `$signed` handling, via
    /// `lower_system_function_call`) for how `width` gets computed. v1
    /// only ever produces this as (or within a concatenation forming) an
    /// assignment's right-hand side -- extension-by-truncation at write
    /// time is all that's needed there. Using `$signed(...)` as an
    /// operand of a comparison (where true signed *ordering*, not just
    /// extension, would be needed) is rejected by the frontend rather
    /// than silently doing an unsigned comparison on the sign-extended
    /// bit pattern -- see `lower_expr`'s `apply_binary_op` for that
    /// guard. `+ & | ^ == != && || !` all happen to already be correct on
    /// a `Signed` operand without any special-casing: two's-complement
    /// addition/bitwise-ops/equality are bit-identical regardless of
    /// whether the operands are "meant" as signed or unsigned, as long as
    /// they're already extended to a common width -- only *ordering*
    /// comparisons, and (once supported) division and arithmetic right
    /// shift, actually need to know.
    Signed(Box<Expr>, u32),
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
    /// Non-blocking write to one element of an array signal
    /// (`mem[i] <= value;` -- see `Signal::depth`). A separate statement
    /// rather than an extra field on `NonBlockingAssign` because the two
    /// really are different writes: this one addresses an element chosen
    /// at simulation time and replaces it whole, where that one addresses
    /// a fixed signal and may write just a constant bit range of it.
    /// Writing a bit range *of an array element* (`mem[i][3:0] <= v;`) is
    /// therefore not representable, and is rejected by the frontend
    /// rather than silently dropping the range.
    ///
    /// `index` is evaluated in the same pre-edge snapshot as `value`,
    /// matching Verilog's non-blocking rule -- so `mem[addr] <= x;` uses
    /// `addr`'s value from *before* this clock edge even if something
    /// else in the same edge assigns to `addr`. An index outside the
    /// array's depth drops the write entirely (there is no element to
    /// update, and 2-state storage has no way to record "this went
    /// nowhere" -- see `ArrayRead` for the reading half of this policy).
    ArrayAssign {
        array: SignalId,
        index: Expr,
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
