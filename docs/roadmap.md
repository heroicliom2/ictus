# Roadmap

*Software/compiler terms (Cranelift, JIT, MTask, AOT) get a short inline
gloss on first use below; full explanations are in
[glossary.md](glossary.md).*

Phases are sequential; don't start a phase's core work before the previous
phase's acceptance bar is met, since later phases depend on the earlier
ones being real (a benchmark suite you can't trust makes every later
performance claim meaningless, an IR that doesn't match real elaboration
needs makes the kernel work throwaway, etc).

## Phase 0 — Benchmark harness

**Before any kernel code.** Pick 3-5 real open cores (PicoRV32, Ibex, a
chunk of CV32E40P or Rocket) as the standing regression/perf suite.

**Acceptance**: harness can run each target design against at least one
reference simulator (Icarus/Verilator) and record pass/fail + timing,
repeatably, in CI (Continuous Integration — automatically building and
testing on every change) or locally. Pillar 1 (speed/lightweight,
decisions.md D11) is just an assumption until this harness exists to
measure it — see docs/architecture.md, Validation strategy.

## Phase 1 — Single-language Verilog simulator

Verilog RTL subset → `ictus-ir` → single-threaded cycle-based engine
(2-state) via Cranelift JIT (compiling the design to native machine code
at startup, in-process, rather than writing out C++ and invoking a
separate compiler — see glossary.md) → VCD or basic FST output.

**Status**: in progress. A tree-walking interpreter (not yet Cranelift --
see decisions.md D12 for why that's the correct order) proves the
frontend → IR → execution pipeline correct on small hand-written designs,
checked both by direct assertions on the lowered IR and by cycle-for-cycle
differential matches against Icarus Verilog:

- `counter.v` (ports, an `if`/`else`-guarded `always @(posedge clk)`
  block, non-blocking assignment, `+`/`!`) --
  `ictus-frontend-verilog/tests/counter.rs`,
  `ictus-cli/tests/differential_counter.rs`.
- `ops_test.v` (internal, non-port `reg` declarations; the binary
  operators `& | ^ == != < <= > >= && ||`; hex/binary literals including
  an underscore digit separator; parenthesized sub-expressions -- which
  caught a real bug: a naive subtree search for an identifier inside a
  parenthesized expression like `(a == b)` was matching `a` alone and
  silently discarding the comparison, fixed by matching `Primary`'s
  variants precisely instead) --
  `ictus-frontend-verilog/tests/ops.rs`,
  `ictus-cli/tests/differential_ops.rs`.

- `comb_test.v` (continuous `assign`/combinational logic, including a
  signal assigned from another signal that's itself a register -- proving
  `ictus_kernel::Simulation`'s two-settle-points-per-`tick()` design
  actually matches a real event-driven simulator, not just that it seemed
  reasonable on paper) -- `ictus-frontend-verilog/tests/comb.rs`,
  `ictus-cli/tests/differential_comb.rs` (which also pins down specific
  values, including an 8-bit-wraparound case and a boundary condition, so
  a future change to the settle order fails loudly here even in the
  unlikely case it somehow still matched Icarus).

- `case_test.v` (plain `case`: a single-value arm, a comma-joined
  multi-value arm, and `default`) -- `ictus-frontend-verilog/tests/case.rs`
  (which also confirms `casez`/`casex` are actively rejected with a clear
  error rather than silently mis-lowered by treating their wildcard bits
  as literal 0/1 -- worth testing the rejection itself, not just trusting
  the code that's supposed to produce it) and
  `ictus-cli/tests/differential_case.rs`.

- `select_test.v` (constant bit-select `data[15]` and part-select
  `data[7:0]`/`data[15:8]`, read side only) --
  `ictus-frontend-verilog/tests/select.rs` and
  `ictus-cli/tests/differential_select.rs`. (`select.rs` also covers a
  constant bit-select/part-select used as an *assignment target* --
  `select_target_test.v` -- and confirms a *variable*-indexed select
  target is still rejected; see "Bit-select/part-select as an assignment
  target" below for the fuller story and why the two cases need different
  treatment.)

- `casez_test.v` (wildcard-bit matching: `4'b1???`, `4'b01??`, mixed with
  a plain exact-match item in the same `casez`) --
  `ictus-frontend-verilog/tests/case.rs::lowers_casez_wildcard_and_exact_arms`
  and `ictus-cli/tests/differential_casez.rs`. This is the payoff for
  `case` and bit-select landing first: real instruction decode
  (`casez (instr[6:0]) 7'b0000???: ...`) needs exactly this combination.
  Caught a real bug along the way -- the wildcard-literal parser only
  marked a written `1` digit as "must match", leaving a written `0` digit
  treated as *also* wildcard (silently matching either 0 or 1 at that
  position) instead of "must match 0"; `lowers_casez_wildcard_and_exact_arms`
  asserts the exact `care_mask` bits and caught it immediately.

- `elseif_test.v` (a 3-rung `else if` chain plus a final `else`) --
  `ictus-frontend-verilog/tests/elseif.rs` (asserts the actual nested
  `Stmt::If` shape, one level per rung, not just that it lowers without
  error) and `ictus-cli/tests/differential_elseif.rs`. No new IR was
  needed -- `else if` is just sugar for a nested `if` inside the previous
  one's `else` branch, built by folding the chain from the last rung
  backward onto the final `else`.

- `concat_test.v` (`{hi, lo}` and a 3-part `{1'b1, hi, lo}` mixing a
  literal with signal references) -- `ictus-frontend-verilog/tests/concat.rs`
  and `ictus-cli/tests/differential_concat.rs`. Concatenation operands
  need each part's bit width known at lowering time to pack correctly
  (`ictus_ir::Expr::Concat` carries `(Expr, u32)` pairs, not bare
  `Expr`s) -- computed by a new `expr_width` helper that only accepts
  operand forms with a statically-known width (literals, signal refs,
  select, nested concatenation), rejecting anything else (arithmetic,
  comparisons) rather than guessing at Verilog's real width-inference
  rules. While implementing this, found and fixed a **pre-existing**
  latent bug (not introduced by this change): a concatenation used as an
  assignment target (`{a, b} <= x;` or `assign {a, b} = x;`) was not
  actually being rejected the way the old doc comments claimed -- the
  identifier search used to find the assignment target deep-searches past
  the concatenation and finds `a` alone, so it would have silently
  lowered the statement as a write to just `a`, discarding `b` and the
  split-assignment semantics with no error at all. Fixed by explicitly
  checking for the `VariableLvalue::Lvalue`/`NetLvalue::Lvalue`
  concatenation-target grammar variants before the identifier search
  runs, in both `lower_nonblocking_assign` and `lower_continuous_assign`;
  `concat.rs::rejects_concatenation_as_assignment_target` tests both.

- `dynsel_test.v` (`data[idx]`, `idx` a signal rather than a literal) --
  `ictus-frontend-verilog/tests/dynsel.rs` and
  `ictus-cli/tests/differential_dynsel.rs` (holds `data` fixed and cycles
  `idx` through all 8 bit positions, checked against Icarus). A new
  `ictus_ir::Expr::DynamicBitSelect` handles the runtime-computed index;
  an out-of-range index (including simply `>= 64`, which would otherwise
  be undefined behavior for a `u64` shift) returns 0 rather than
  panicking -- a deliberate, documented choice given the kernel is
  2-state only and has no 'x' to propagate the way a 4-state reference
  simulator would, tested directly in `ictus-kernel`'s own unit tests
  rather than differentially for exactly that reason (it wouldn't, and
  shouldn't be expected to, match Icarus's 'x' output for that case).
  Indexed *part*-select (`x[base +: width]`, a fixed width at a variable
  base) is still not supported -- only single-bit variable select.

**First real attempt at lowering picorv32 itself** (not a hand-written
fixture -- `bench/designs/picorv32/picorv32.v`, already vendored for phase
0): this surfaced several real gaps directly, each fixed and tested the
same way as everything above (fixture, structural test, differential test
against Icarus) rather than left as a guess:

- Ports that inherit their direction from the previous one in the list
  (`input clk, resetn,` -- picorv32's own first two ports, verbatim) --
  `lower_port` now tracks and propagates the last explicit direction
  instead of erroring.
- A single declaration naming several signals (`reg a, b, c;` -- picorv32
  does this too) -- `lower_internal_signal` now collects every declared
  name in the declaration (found by searching for the grammar's own
  `NetIdentifier`/`VariableIdentifier` "this is a declared name" markers,
  not a blind search for any identifier, which could wrongly also match a
  name inside an initializer expression like `reg x = Y;`), not just the
  first.
- The ternary operator (`cond ? a : b`), not supported at all before this
  -- new `ictus_ir::Expr::Ternary`. While adding it, found and worked
  around a genuine `sv-parser` bug: an *unparenthesized* ternary right
  after a binary operator's right operand (`a > c ? a : c`, extremely
  common style, used throughout picorv32) gets mis-parsed as if it were
  `a > (c ? a : c)` instead of the only correct reading, `(a > c) ? a :
  c`. See decisions.md D13 for the full story and the fix.
- (`decl_style_test.v`/`decl_style.rs`/`differential_decl_style.rs` cover
  the first two together with nested nested ternaries;
  `ternary_precedence_test.v`/`ternary_precedence.rs`/
  `differential_ternary_precedence.rs` cover the precedence fix in
  isolation.)
- Also caught and fixed a process gap, not a code gap: three fixes landed
  before their tests were written, chasing the picorv32 diagnostic output
  turn by turn -- corrected by writing the full fixture/structural/
  differential coverage for all three before moving on, per
  docs/decisions.md's own established practice for this project.

**Module parameters** (`#(parameter [7:0] X = 1, ...)`): picorv32
references its own parameters (`COMPRESSED_ISA`, etc.) directly in
expressions throughout the design body -- fixed. A parameter isn't a
signal: `lower_parameters` resolves every parameter's default value (via
Verilog's own separate *constant*-expression grammar, the same one
already used for bit-select/part-select bounds -- not the general
`lower_expr` path, so a default can't yet reference another parameter,
not needed by picorv32's own parameters) to a plain integer at lowering
time, and every reference to a parameter is substituted directly into the
expression tree as `Expr::Literal` -- so `ictus_ir::Module` and the
kernel never need to know parameters exist at all. This required
threading a small `Ctx` (module + resolved parameter table) through
expression lowering in place of a bare `&Module` reference, since
`lower_primary`'s identifier resolution now needs to check both.
Verified per this project's usual practice: `param.rs` asserts every
parameter reference in the fixture resolved to the correct literal value
(not just that lowering succeeded), and `differential_param.rs` drives
several cycles against Icarus Verilog, including the wraparound behavior
the two parameters together produce.

**Bit-select/part-select as an assignment target** (`mem_rdata_q[14:12]
<= 3'b000;` -- picorv32 does this constantly for its instruction-decode
registers): done, for the common case. This was meaningfully bigger than
the read-side support already in place, because a partial-width write
needs *read-modify-write* semantics -- write just the selected bits,
leave the rest of the signal's current stored value alone -- which the
kernel had no representation for at all (`Stmt::NonBlockingAssign` was
always a full-width replace).

- `ictus_ir::Stmt::NonBlockingAssign` grew a `target_range: Option<(u32,
  u32)>` field -- `None` for a plain full-width target (the overwhelming
  common case, unchanged), `Some((msb, lsb))` for a constant
  bit-select/part-select target.
- `ictus_kernel`'s `tick()` commit loop now branches on that field: for
  `Some((msb, lsb))`, it clears just those bits of the signal's *current*
  stored value and ORs in the new bits shifted into position, instead of
  replacing the whole word. Multiple partial writes to the same signal in
  one tick (picorv32 does this too -- disjoint field writes to the same
  register in one `always` block) are applied in declaration order via
  direct mutation of the live value array, which is correct (not a race
  with the non-blocking-read rule above) because every right-hand side
  across the whole tick is already evaluated, against the pre-tick
  snapshot, before this commit loop starts mutating anything -- see the
  commit loop's own comment in `ictus-kernel/src/lib.rs`.
- The frontend's old `reject_select_target` (which unconditionally
  errored on *any* select target) became `lower_select_target_range`,
  which extracts `(msb, lsb)` for a constant bit-select/part-select target
  instead. A **variable**-indexed target (`x[i] <= v;`) and an indexed
  part-select target (`x[base +: width] <= v;`) are still rejected --
  correctly: the kernel's write range has to be known at lowering time,
  not recomputed per cycle, so supporting those would need a genuinely
  different (and not yet designed) kernel representation, not just a
  frontend change. A select as a *continuous*-assignment target (`assign
  x[7:0] = v;`) is also still rejected, deliberately: `ictus_ir::Assign`
  has no `target_range` equivalent, since continuous assignment has no
  commit phase to do a read-modify-write in the way `tick()` does for
  `<=` -- doing this properly would mean reworking
  `settle_combinational`'s single-pass-replace design, out of scope here.
- Verified per this project's usual practice: `select_target_test.v`
  (`ictus-frontend-verilog/tests/select.rs` for the structural shape,
  `ictus-cli/tests/differential_select_target.rs` against Icarus Verilog)
  drives two *disjoint* partial writes to the same register in one clock
  edge -- one field self-incrementing and wrapping at its own (narrower)
  width, the other loading from an input -- specifically to exercise the
  same-signal-multiple-partial-writes case, not just a single isolated
  partial write. `ictus-kernel`'s own unit tests additionally check the
  read-modify-write and same-tick-combination behavior in isolation from
  the frontend (`partial_writes_to_disjoint_ranges_combine_in_one_tick`,
  `partial_write_wraps_within_its_own_width_not_the_full_signal`).

**Concatenation-of-selects as an assignment target**
(`{mem_rdata_q[31:25], mem_rdata_q[11:7]} <= {...};` -- picorv32 uses this
for its instruction-decode registers): done. Rather than teaching the
kernel yet another representation, this needed *no* new IR at all: a
concatenation target is split, entirely at lowering time, into one plain
`Stmt::NonBlockingAssign` per part -- each part gets the slice of the
right-hand side that lines up with its position (the leftmost part is the
most-significant bits, the same packing order a concatenation
*expression* already uses on the read side). The slicing itself reuses
`Expr::Select` (also not new): each part's value is the shared,
lowered-once right-hand-side expression, cloned and wrapped in a
`Select` for that part's bit range -- safe to evaluate more than once
per tick since a non-blocking assignment's right-hand side has no side
effects to duplicate.

- New `lower_concat_target_assign` in `ictus-frontend-verilog` handles
  the `VariableLvalue::Lvalue` (concatenation-target) case that
  `lower_nonblocking_assign` used to reject outright; each part goes
  through the same `lower_select_target_range` the plain single-select
  case uses, so a part can be a whole signal (`target_range: None`) or a
  constant bit-select/part-select (`Some((msb, lsb))`) -- picorv32 mixes
  both in the same statement. A **nested** concatenation inside the
  target (`{a, {b, c}} <= v;`) is rejected, not guessed at: splitting a
  value across a nested group would need this same slicing logic applied
  recursively, a real gap rather than a silent-wrong-answer risk.
  `lower_nonblocking_assign` itself now returns `Vec<Stmt>` instead of a
  single `Stmt`, since one source statement can lower to several.
- Verified per this project's usual practice, with two fixtures: a simple
  one (`concat_nonblocking_target_test.v`, both parts plain whole
  signals -- this is the exact shape the pre-existing
  silently-writes-only-the-first-part bug, fixed earlier this phase,
  would have hit) and one mirroring picorv32's own style directly
  (`concat_target_select_test.v`, parts that are constant selects of the
  *same* signal mixed with a plain signal, right-hand side itself a
  concatenation) -- both checked structurally
  (`ictus-frontend-verilog/tests/concat.rs`) and, for the harder one,
  differentially against Icarus Verilog
  (`ictus-cli/tests/differential_concat_target_select.rs`, a nibble-swap
  design chosen so the expected output is easy to hand-verify). Plus a
  negative test confirming the nested-concatenation case is rejected with
  a clear error, and confirming a concatenation is still rejected as a
  *continuous*-assignment target (`assign {a,b} = v;` -- `ictus_ir::Assign`
  still has no commit phase to split a value across multiple targets in,
  same reasoning as the plain-select case above).

**The `$signed(...)` system function**: done, for the case picorv32
actually needs -- sign-extending a narrower value into a wider assignment
target (`decoded_imm <= $signed(mem_rdata_q[31:20]);`,
`mem_rdata_q[31:20] <= $signed({mem_rdata_latched[12],
mem_rdata_latched[6:2]});`). Scoped deliberately narrower than full
Verilog `signed` semantics:

- New `ictus_ir::Expr::Signed(Box<Expr>, u32)` marks its operand as
  signed, carrying the operand's own natural width (computed by the
  already-existing `expr_width` helper) so the kernel knows where the
  sign bit is. It's a no-op on the operand's own bits -- evaluating it
  just replicates that bit upward through every bit above its declared
  width (real two's-complement sign extension) rather than the implicit
  zero-extension every other expression gets; whatever narrower width
  actually gets *used* downstream (a `Select`'s own masking, or the
  kernel's commit-time write mask for the assignment target) then keeps
  only however many of those bits it needs. This composes for free with
  everything built in the previous two entries: no special-casing was
  needed in `lower_concat_target_assign` for a concatenation target whose
  right-hand side is `$signed(...)` (picorv32's
  `{decoded_imm_j[...], ...} <= $signed({...});` style) -- it already just
  clones and slices whatever `Expr` the right-hand side lowered to.
- New `lower_system_function_call` (called from `lower_primary`) handles
  `$signed(expr)` specifically and rejects every other system
  function/task call. `+ & | ^ == != && || !` all accept a `Signed`
  operand safely with no special-casing (two's-complement
  addition/bitwise-ops/equality are bit-identical regardless of declared
  signedness, once operands are extended to a common width) -- but a
  `Signed` operand of an *ordering* comparison (`< <= > >=`) is rejected
  by a new guard in `apply_binary_op`, since real signed ordering needs a
  genuinely different comparison (not implemented) and silently falling
  back to unsigned comparison on the sign-extended bit pattern would be
  exactly the kind of silent-wrong-answer this project treats as the
  worst failure mode -- picorv32's ALU does exactly this
  (`alu_lts <= $signed(reg_op1) < $signed(reg_op2);`), so this rejection
  is confirmed to fire on real source, not just a hypothetical.
- Verified per this project's usual practice: a plain-reference fixture
  (`signed_ext_test.v`) and one mirroring picorv32's own concatenation
  style directly (`signed_concat_test.v`), both checked structurally
  (`ictus-frontend-verilog/tests/signed.rs`) and differentially against
  Icarus Verilog with values chosen to include both a sign-bit-clear and
  several sign-bit-set cases (`ictus-cli/tests/differential_signed_ext.rs`,
  `differential_signed_concat.rs`) -- a zero-extension bug would only be
  visible on the sign-bit-set cases, so those are the ones that actually
  exercise the feature, not just "does it lower without erroring."
  `ictus_kernel`'s own unit tests isolate the sign-extension bit
  manipulation from the frontend entirely. Plus a negative test
  confirming the signed-ordering-comparison rejection fires with a clear
  error, using a fixture shaped exactly like picorv32's own `alu_lts`
  line.

**Task-call statements**: done, resolving the fork this section previously
left open in favor of the conservative option -- a call is only accepted
when the *called task's own body* is provably empty, not generalized to
"any zero-argument task call is a no-op." picorv32's own `` `assert(assert_expr) ``
is `` `define ``-d (picorv32.v line 47) to expand to a call to
`empty_statement;`, a deliberately-empty task (line 214, written as
`begin end`) used as a no-op placeholder when formal-verification
assertions are compiled out.

- New `lower_task_declarations` scans every `task ... endtask` in the
  module up front (same pattern as `lower_parameters`) and records the
  names of tasks whose body is *provably empty* -- every top-level
  statement is a no-op via a new recursive `statement_is_noop` helper,
  which treats a null statement (`;`) or a `begin...end` block whose own
  statements are all, recursively, no-ops as empty. The recursion into
  `begin...end` matters: picorv32's `empty_statement` body is literally
  `begin end`, one block statement containing nothing, not zero
  statements at the task's own top level -- checking only
  `Vec<StatementOrNull>::is_empty()` at that one level would have missed
  it and wrongly rejected the exact case this feature exists for.
- New `lower_task_call_statement` (wired into `lower_statement_item`)
  handles `some_task;` -- a call with any arguments at all is rejected
  outright (v1 has no notion of task ports to bind them to), and a call
  to a name not in the empty-task set is rejected with the same error
  whether that's because the task has real statements in it or because no
  such task exists. An accepted call lowers to `Vec::new()`: zero
  `ictus_ir::Stmt`s, a true no-op, needing no new IR at all.
- Verified per this project's usual practice: a fixture mirroring
  picorv32's own style directly (`task_call_test.v`, an empty
  `begin end` task called in the middle of a counter's clocked process)
  checked both structurally (the call contributes zero statements --
  `ictus-frontend-verilog/tests/task_call.rs`) and differentially against
  Icarus Verilog (the counter's behavior is unaffected by the call --
  `ictus-cli/tests/differential_task_call.rs`), plus two negative fixtures
  confirming a task with a real (non-empty) body and a task call with an
  argument are both still rejected.

**Replication/multiple concatenation** (`{4{1'b0}}`, i.e. Verilog's
`{N{expr}}` repeat-concatenation syntax -- `sv_parser::Primary::
MultipleConcatenation`, distinct from the plain `{a,b}` concatenation
already supported): done, confirming the guess in the previous version of
this section -- a natural extension of the existing machinery, not a new
design fork the way task-call statements were. picorv32 uses it three
times, all real usage sites read directly rather than guessed at: a
1-bit flag replicated to a byte-wide enable mask
(`mem_wstrb <= mem_la_wstrb & {4{mem_la_write}};`) and two cases
replicating a 16-/8-bit field to build a wider write-data word
(`mem_la_wdata = {2{reg_op2[15:0]}};`).

- New `lower_multiple_concatenation` needs no new IR: the replication
  count must fold to a compile-time constant (`lower_expr`'s result must
  be `Expr::Literal`, the same restriction a bit-select/part-select bound
  already has -- a signal-dependent count is rejected, not deferred to
  runtime, since `Expr::Concat`'s part list has to be a fixed size at
  lowering time), and the result is the inner `{...}`'s own parts
  (lowered exactly like any other concatenation via the existing
  `lower_concatenation`), physically cloned and repeated `N` times in a
  row into one flat `Expr::Concat` -- as if the source had written that
  many literal copies of `{...}` back to back. A count of `0` (legal
  Verilog for a deliberate zero-width contribution) is rejected in v1,
  same reasoning as a plain empty `{}` already being rejected:
  `Expr::Concat` can't represent an empty part list.
- Verified per this project's usual practice: a fixture covering both
  real shapes picorv32 uses -- a single-expression replication
  (`{8{flag}}`, mirroring the `mem_wstrb`/`mem_la_write` style) *and*
  replicating a multi-part inner concatenation (`{2{a, b}}`, mirroring
  the `mem_la_wdata` style, and a genuinely different code path through
  `lower_multiple_concatenation` than the single-expression case) --
  checked structurally (`ictus-frontend-verilog/tests/replicate.rs`) and
  differentially against Icarus Verilog
  (`ictus-cli/tests/differential_replicate.rs`). Plus two negative tests:
  a non-constant count (a signal, not a literal) and a count of `0`, both
  confirmed rejected with a clear, specific error.

**Unary bitwise/reduction operators**: done. Re-running the picorv32
diagnostic after the replication fix hit `~&` (reduction NAND) as the
next unsupported unary operator -- `lower_expr`'s `E::Unary` arm only
recognized logical `!` before this. Verilog's unary operators on a
vector operand also include plain bitwise `~` (complement) and six
*reduction* operators (`&`, `|`, `^`, `~&`, `~|`, `~^`/`^~`) that fold an
entire multi-bit operand down to a single bit -- a materially different
operation from `~`, not a variant of it, so all of them were implemented
together as one coherent unit rather than chasing them one at a time.

- Four new `Expr` variants: `BitwiseNot(Box<Expr>, u32)`, `ReduceAnd`,
  `ReduceOr`, `ReduceXor` (each `(Box<Expr>, u32)`), all carrying the
  *operand's* width -- needed at evaluation time (to know where to stop
  complementing bits, or how many bits to fold over), computed via the
  existing `expr_width` helper at lowering time, same pattern `Expr::
  Signed` already established. Reduction NAND/NOR/XNOR (`~& ~| ~^`/`^~`)
  get **no dedicated variant**: they're lowered as the corresponding
  reduction wrapped in a 1-bit `BitwiseNot` (`~&x` becomes
  `BitwiseNot(ReduceAnd(x, width), 1)`), reusing `BitwiseNot`'s own
  correct-by-construction bit-complement-and-mask behavior on a
  known-1-bit value instead of adding three more near-identical variants.
- `ictus_kernel::eval_expr` masks `BitwiseNot`'s complement to its
  declared width immediately (`mask(!eval_expr(inner), width)`) rather
  than leaving the high bits of the underlying `u64` set and relying on
  masking happening later at signal-write time the way `And`/`Or`/`Xor`/
  `Add` safely can (those operators never introduce a stray 1 bit above
  the operands' own width; complementing does). `ReduceAnd`/`Or`/`Xor`
  mask the operand first, then fold: all-ones-compare, nonzero-check, and
  `count_ones() % 2` respectively.
- Verified per this project's usual practice: one fixture exercising the
  whole family on the same 4-bit input (`!x ~x &x |x ^x ~&x ~|x ~^x`),
  with `~&x` written in the exact style picorv32 uses
  (`~&mem_rdata_latched[1:0]`) -- checked structurally
  (`ictus-frontend-verilog/tests/unary_ops.rs`, confirming both the
  correct `Expr` shape for each operator and, for the composed NAND/NOR/
  XNOR forms, the `BitwiseNot`-wrapping-a-reduction shape specifically)
  and differentially against Icarus Verilog across five input values
  chosen to exercise all-zeros, all-ones, and both odd/even bit-parity
  cases (`ictus-cli/tests/differential_unary_ops.rs`). `ictus_kernel`'s
  own unit tests additionally isolate the masking and folding behavior
  from the frontend.

**Next confirmed blocker**: `localparam`, and with it, general
compile-time constant-expression evaluation -- a materially bigger lift
than the last several increments, not a natural one-line extension of
what exists. Re-running the picorv32 diagnostic after the unary-operator
fix hits `regindex_bits`, an unresolved identifier, because
`lower_parameters` only ever walks `RefNode::ParameterDeclarationParam`
(`#(parameter ...)`) -- `localparam` is a *separate* grammar production
this frontend has never looked for at all. Worse, the specific value
expression that broke it needs three things at once, none supported by
`lower_constant_param_value`'s narrow constant-expression walk (built
only to pull a single `Number` out of a `parameter`'s default):

```verilog
localparam integer regindex_bits =
    (ENABLE_REGS_16_31 ? 5 : 4) + ENABLE_IRQ*ENABLE_IRQ_QREGS;
```

- **Cross-parameter references**: `lower_parameters`'s own doc comment
  already flags this as an explicit, known-but-unverified gap ("a
  parameter default referencing another parameter isn't supported...not
  needed by picorv32's own parameters" -- that assumption just broke:
  `regindex_bits` references three other parameters directly).
- **The ternary operator** inside a constant expression.
- **Multiplication** (`*`) -- not an operator `apply_binary_op` handles
  at all yet, constant-context or otherwise; the first arithmetic
  operator beyond `+` this frontend would need.

Solving this properly likely means `lower_constant_param_value` (or a
successor) needs to route through something much closer to the general
`lower_expr` -- with parameters already resolved to `Expr::Literal`
available for reference the way `lower_primary` already checks
`module.parameters` -- rather than staying a special-purpose
single-`Number` extractor, plus a real decision about how `localparam`
fits into `Ctx`/`lower_parameters`'s existing structure (same table as
`parameter`, since both resolve to plain constants the kernel never
sees? a separate pass?) and whether/how multiplication generalizes
beyond just constant-folding into the *general* expression grammar
`lower_expr` handles for signal-valued (non-constant) arithmetic too.
Worth a real decision (and likely a decisions.md entry) before picking
an approach, not a default -- not yet attempted.

The supported language subset is still intentionally narrow: single
ANSI-style module, any number of clocked processes and `assign`s but no
`always_comb`, module `parameter`s (not `localparam`; defaults must be
constant literals, no overriding at instantiation, no cross-parameter
references, no operators beyond what a bare literal needs), constant/
variable bit-select and constant part-select, concatenation (plain and
replication) and ternary on reads only, a constant bit-select/part-select
-- or a concatenation of such -- as a non-blocking (`<=`) assignment
target but not a continuous (`assign`) one and not with a variable
index, `$signed(...)` to sign-extend a value into a wider assignment
target but not as an operand of an ordering comparison (and no other
system function), a call to a provably-empty task but no other
task/function calls, logical `!`, bitwise `~`, and the reduction
operators, but no multiplication or any other arithmetic operator beyond
`+`, no array/memory signals (`reg [31:0] mem [0:31]` -- this is what
picorv32's register file actually needs, and is a distinct, likely-larger
gap from bit-select on a single signal), no module instantiation.
Cranelift codegen and actually getting picorv32 fully through the
pipeline are both still ahead of where this stands today.

**Acceptance**: benchmark suite from phase 0 runs correctly (differential
match against a reference simulator) and timing is recorded as a baseline.

## Phase 2 — Static multi-threaded partitioning

Partition the dataflow graph (design represented as operations-and-their-
dependencies, not a flat statement list) into independent clusters at
elaboration time, schedule statically across threads — decided once, in
advance, not renegotiated at runtime (MTask-style, Verilator's term for
this; see decisions.md D5).

**Acceptance**: measurable speedup on the phase 0 benchmark suite from
multithreading, without correctness regressions.

## Phase 3 — SystemVerilog RTL constructs + native testbench layer

Interfaces, packed structs, enums — **not** classes/UVM. Plus the
interpreted execution path for non-synthesizable testbench code (`initial`,
`#delay`, `fork`/`join`) that runs alongside the compiled RTL engine (D2,
D4).

**Acceptance**: a plain SV testbench (no UVM) can drive a DUT end-to-end
without a C++/SystemC harness.

## Phase 4 — VHDL frontend + mixed-language elaboration

VHDL frontend built on `vhdl_lang` (D8). Cross-language port binding and
signal type/width compatibility checking for mixed VHDL/Verilog
instantiation — the hardest part of this phase (see docs/architecture.md,
Elaboration).

**Acceptance**: a mixed VHDL+Verilog design elaborates and simulates
correctly.

## Phase 5 — Waveform + debug UI

FST output via `wellen`/clean-room writer (D7, never gtkwave's C core).
Integrate with Surfer rather than building a viewer.

**Acceptance**: Surfer can open and correctly render a waveform produced by
Ictus.

## Phase 6 — Performance passes (stretch)

Optional AOT (Ahead-Of-Time — compile fully as a separate step before
running, trading slower builds for faster execution) path via `rustc`/LLVM
for max-throughput CI runs, alongside the Cranelift JIT default (D3).
Opt-in 4-state fidelity mode. Possibly gate-level/SDF if there's a
deliberate reason to go there (D10).

## Deferred beyond phase 6

UVM, constrained-random, functional/code coverage, full SVA, IP encryption
are real long-term goals (decisions.md D11, pillar 3), not rejected scope —
they're just not part of phases 0-6. Pull one forward only with an explicit
check against pillars 1-2 (speed/lightweight, native mixed-language support
with no C/C++ model requirement) and a new decisions.md entry explaining
why it can be added now without compromising them.
