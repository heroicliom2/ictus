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

**`localparam` and general compile-time constant-expression evaluation**:
done -- the materially bigger lift the previous version of this section
flagged, not a natural one-line extension of what existed, and it took a
real design pass rather than a default. Closes `regindex_bits` and every
other `localparam`/cross-parameter-reference gap found in picorv32 in one
pass, not just the one line that first surfaced it:

```verilog
localparam integer regindex_bits =
    (ENABLE_REGS_16_31 ? 5 : 4) + ENABLE_IRQ*ENABLE_IRQ_QREGS;
```

- `lower_parameters` now resolves `#(parameter ...)` *and* `localparam`
  in one combined pass, into one shared name table -- both are just
  named compile-time constants once instantiation-time overriding is out
  of scope (as it already was), so there was no real reason to keep them
  separate. Walking the whole module once, in source order, growing the
  same table as it goes, is what makes cross-references work at all: a
  `localparam` can reference an *earlier* `parameter` simply because the
  `#(parameter ...)` port list is always visited first, by source order
  -- a pragmatic "declaration precedes use" assumption, not a real
  dependency solve, but correct for every real case found so far.
- New `lower_constant_expr`/`lower_constant_primary` walk Verilog's
  *constant*-expression grammar (`ConstantExpression`/`ConstantPrimary`
  -- a genuinely separate parallel hierarchy from the general
  `Expression`/`Primary` grammar, required wherever Verilog demands a
  compile-time constant: parameter/localparam values, and packed-range
  bounds) structurally for the first time, building an ordinary
  `ictus_ir::Expr` via the *same* `apply_binary_op` the general grammar
  uses -- one definition of what each operator means, not two. This is
  also where multiplication (`Expr::Mul`) and subtraction (`Expr::Sub`)
  were added as real operators, alongside the existing `+`, since the
  general expression grammar needed `-` too (see below) and there was no
  reason to make `*`/`-` constant-only.
- New `try_const_fold(&Expr) -> Option<u64>` folds a closed (no signal
  reference) `Expr` tree down to a plain value -- deliberately exhaustive
  over every `Expr` variant (no wildcard arm), mirroring
  `ictus_kernel::eval_expr`'s own logic (duplicated across the crate
  boundary on purpose: constant folding is a frontend/elaboration-time
  concern, evaluation is the kernel's runtime concern -- different
  phases, so the overlap is expected, not a sign the crates should share
  code). This one helper is reused everywhere a compile-time constant is
  now required: a `parameter`/`localparam` default, a packed-range bound
  (`lower_packed_range`, now threaded with the parameter table so
  `reg [regindex_bits-1:0] decoded_rd, decoded_rs1;` resolves correctly),
  a constant part-select bound (`lower_constant_index`, unified onto the
  same walk instead of keeping its own separate, less capable one), and
  -- the case that needed a genuine design decision, not just plumbing --
  a bit-select *target*'s index lowered through the *general* expression
  grammar (`lower_select_target_range`'s single-bit arm), which is how
  `decoded_rs1[regindex_bits-1] <= 1;` resolves: `regindex_bits` becomes
  `Expr::Literal` via `lower_primary`'s existing parameter fallback, so
  the whole `Expr::Sub(Literal, Literal)` tree `lower_expr` builds for
  the index is foldable, even though the same code path would just as
  happily build `Expr::Sub(Ref(signal), Literal)` for a genuinely
  non-constant index (correctly left as `None`, still rejected). The
  read-side single-bit-select (`lower_select`) picked up the identical
  upgrade for consistency, replacing its old "must literally already be
  `Expr::Literal`" check with the same fold.
- Confirmed by empirically debugging *through* sv-parser's own grammar,
  not just picorv32's, along the way: a **bare parameter reference with
  no parentheses at all** (picorv32's own `ENABLE_REGS_16_31`, 17
  characters) can parse as `ConstantPrimary::ConstantFunctionCall`
  instead of `::PsParameter` -- a real sv-parser grammar ambiguity (the
  same *kind* of finding as D13's ternary-precedence bug, though not the
  same bug). Confirmed there's no actual `function` anywhere in
  picorv32.v, so a zero-argument "constant function call" is always
  really just a parameter reference the parser classified differently;
  handled by treating it exactly like `PsParameter` (a *non*-zero-argument
  one is rejected -- that would be a genuine constant function call,
  which v1 doesn't implement at all).
- Also needed, once real picorv32 `localparam`s were actually run through
  this: logical `||` (picorv32's `WITH_PCPI = ENABLE_PCPI || ENABLE_MUL
  || ...;`) and concatenation (picorv32's `TRACE_BRANCH = {4'b 0001,
  32'b 0};`) inside a constant expression -- both fall out of the same
  `try_const_fold`/`lower_constant_primary` design already described
  above, not separate features.
- Verified per this project's usual practice, with a fixture chosen to
  hit every real shape found (cross-parameter reference, ternary,
  multiplication, logical OR, a packed-range bound referencing a
  localparam, and a bit-select *target* index referencing one) --
  checked structurally (`ictus-frontend-verilog/tests/localparam.rs`)
  and, most importantly, differentially against Icarus Verilog
  (`ictus-cli/tests/differential_localparam.rs`): Icarus independently
  computes the same constant expressions from scratch, so an exact trace
  match is strong evidence the arithmetic/ternary/cross-reference
  resolution is bit-for-bit correct, not just "didn't error." A separate
  fixture covers the concatenation-value case
  (`localparam_concat_test.v`), and a negative test confirms a reference
  to an undeclared parameter is rejected with a clear error rather than
  silently treated as 0.

**A 4-state `x`/`z` digit in a literal outside a `case`/`casez`/`casex`
item**: done, resolving the policy question the previous version of this
section deliberately left open rather than defaulted. picorv32 uses this
constantly, in two distinct real styles: an all-`x` "don't care" output
on a dead/inactive code path (`assign pcpi_mul_rd = 32'bx;`) and a
"default to `x`, then override in every `case` arm" idiom
(`decoded_imm <= 1'bx;` followed by a `case` that sets a real value for
every instruction encoding that matters).

- **Decision**: an `x`/`z` digit -- whole-value or mixed with real digits,
  in any base -- resolves to the bit `0`. Matches Verilator's own default
  X-handling policy (a real precedent for this exact choice, confirmed,
  not assumed) and needs no change to this kernel's 2-state signal
  *representation* (decisions.md D6 is about something larger and still
  future -- real per-signal 4-state tracking -- this is a much smaller,
  self-contained literal-*parsing* policy). New `parse_binary_literal_value`/
  `parse_hex_literal_value` replace the old `u64::from_str_radix` calls
  (which simply failed to parse an `x`/`z` character at all) with a
  digit-by-digit walk -- deliberately *not* sharing code with
  `lower_wildcard_binary` (case-item wildcard matching, which tracks a
  `care_mask`, a different job). `DecimalNumber::BaseXNumber`/
  `BaseZNumber` (a whole-value-only 4-state decimal literal, `8'dx`,
  `8'dz`) got the same policy applied for consistency, replacing their
  previous explicit rejection.
- **Why this isn't (and can't be) verified with a general differential
  test**: a genuinely-`x` result in a *real* 4-state simulator like
  Icarus has no single comparable value at all, so comparing Ictus's
  resolved-to-0 output against Icarus's reported `x` for the *same*
  signal would either fail to parse as a number or just disagree by
  design -- not a bug on either side, just two different, both-valid
  answers to a question standard Verilog leaves open. Verified instead
  with a structural test (`ictus-frontend-verilog/tests/xz_literal.rs`)
  confirming the resolved value directly, *and* a differential test built
  around the specific real idiom where this is actually safe to compare
  end-to-end: the "default to `x`, then override in every case arm"
  pattern, where the `x` default is provably never the value actually
  observed in *either* simulator
  (`ictus-cli/tests/differential_xz_default.rs`).

**Comparison/logical/reduction results as concatenation operands**:
also done, found immediately after the fix above (`{a[14:12]==3'b0, ...}`
-style instruction-decode concatenations, common once picorv32 could be
run further through the pipeline). `expr_width` previously only knew
`Select`/`DynamicBitSelect`/`Signed`/`BitwiseNot`/reduction results'
widths -- extended to also recognize `Not`/`Eq`/`Ne`/`Lt`/`Le`/`Gt`/`Ge`/
`LogicalAnd`/`LogicalOr` as always exactly 1 bit, which Verilog defines
them to be (not an approximation the way `Add`/`Sub`/`Mul`'s width would
be, so this is a safe, unambiguous addition, unlike guessing at general
arithmetic width inference). Verified with a fixture concatenating four
different comparison results, checked structurally
(`ictus-frontend-verilog/tests/concat_compare.rs`) and differentially
against Icarus Verilog across several `(a, b)` pairs chosen to exercise
each comparison
(`ictus-cli/tests/differential_concat_compare.rs`).

**Shift operators** (`<<`, `>>`, `<<<`, `>>>`): done, and the guess in
the previous version of this section held up -- it did extend
`apply_binary_op`'s existing `Signed`-operand guard rather than needing a
new mechanism. Three new `Expr` variants (`Shl`, `Shr`, `AShr`), and the
whole design turns on the same signed/unsigned axis that guard already
reasons about, splitting three ways:

- `<<` and `<<<` are **sign-agnostic** -- shifting left has no sign
  behavior to differ about, so both lower to the same `Expr::Shl` and a
  `Signed` operand passes through untouched, exactly like `+`/`-`/`*`.
- `>>` is a **logical** shift in Verilog even for a signed operand
  (that difference is precisely why `>>>` exists) -- which an
  `Expr::Signed` value can't represent, since it arrives already
  sign-extended across all 64 bits and those extension bits would shift
  down in place of the zeros Verilog specifies. So `$signed(x) >> n` is
  *rejected*, same discipline as a signed ordering comparison. No real
  design has needed it: picorv32 writes `>>` only on plain unsigned
  operands.
- `>>>` is the one that genuinely needs the distinction, and the one
  place a `Signed` operand is actively *required* rather than merely
  tolerated. With one, it lowers to `Expr::AShr`, evaluated as a plain
  `i64` shift -- correct precisely *because* the operand arrives
  sign-extended, so the bit an `i64` shift replicates is the operand's
  real sign bit rather than whatever landed in bit 63. Without one,
  Verilog itself defines `>>>` as an ordinary logical shift, so it
  lowers to `Expr::Shr` -- the LRM's own rule, not an approximation.
  (The `Signed` check is deliberately shallow, matching the existing
  ordering-comparison guard: `($signed(a) + 1) >>> n` wouldn't be
  detected. picorv32 always writes the direct `$signed(...) >>> n` form,
  so no real case is affected; documented at the guard rather than
  silently assumed away.)

Out-of-range shift amounts needed care the obvious Rust spelling gets
wrong: a bare `<<`/`>>` on a `u64` panics on shift-overflow, and
`wrapping_shl` would silently take the amount modulo 64 -- turning
`x << 64` into `x << 0`, i.e. `x` unchanged, when Verilog says every bit
is shifted out. Logical shifts of 64 or more produce 0; an arithmetic one
clamps to 63, which already replicates the sign bit across every bit.

Verified per this project's usual practice, with values chosen so the
sign bit is set in three of four cases -- so an arithmetic and a logical
right shift give visibly different answers and a regression that lowered
`>>>` as a logical shift would fail rather than slip through -- plus one
sign-bit-clear control case where all three right shifts must agree.
Checked structurally (`ictus-frontend-verilog/tests/shift.rs`, including
that `>>>` on an unsigned operand really does lower to a logical `Shr`,
and a negative test for the rejected `$signed(x) >> n`) and
differentially against Icarus Verilog
(`ictus-cli/tests/differential_shift.rs`), which also covers the same
`$signed(x) >>> n` expression assigned into a *wider* target, where
Verilog sign-extends to the context width first and the extra bits must
come out as sign bits rather than zeros. The out-of-range shift amounts
are covered by `ictus_kernel`'s own unit tests instead -- a realistic
design's shift-amount signal is only a few bits wide, so no differential
fixture can reach them.

**Signed ordering comparisons** (`$signed(a) < $signed(b)`): done,
finally reaching and resolving the gap D16 deliberately deferred back
when `$signed(...)` was first added. picorv32's ALU writes
`alu_lts <= $signed(reg_op1) < $signed(reg_op2);` -- and that turned out
to be the *only* signed-comparison shape in the whole design, with both
operands signed, which made the scope clean. As the previous version of
this section guessed, the fix was small once `Expr::Signed` and
`Expr::AShr` had established the pattern:

- One new `Expr::SignedLt` variant, not four. The other three orderings
  are composed from it at lowering time by swapping operands and/or
  negating (`a > b` is `b < a`; `a <= b` is `!(b < a)`; `a >= b` is
  `!(a < b)`) -- identities that hold exactly for integers, and the same
  "compose rather than add near-identical variants" approach
  `~&x`/`ReduceAnd` and unsigned-`>>>`/`Shr` already take.
- Evaluated as a plain `i64` comparison, correct for the same reason
  `AShr`'s `i64` shift is: both operands arrive already sign-extended
  across all 64 bits, so the sign an `i64` comparison reads is the
  operand's real one rather than whatever landed in bit 63.
- The *mixed* case the previous version of this section flagged as the
  real open question got a definite answer rather than a default:
  Verilog's own rule is that a comparison with one unsigned operand is
  performed **unsigned**, but doing that correctly needs the signed
  operand truncated back to its own width first (an 8-bit -1 has to read
  as 255, not as the 64-bit sign-extended pattern `Expr::Signed`
  evaluates to). That truncation isn't implemented and no real design
  needs it, so the mixed case is rejected with an error that says so --
  not silently compared as an enormous positive number. The shallow
  `Signed`-check limitation carries over unchanged from the existing
  guard, documented in place.

Verified with a fixture covering all four orderings plus an unsigned
control on the *same* operands, driven with values where exactly one
sign bit is set -- so the signed and unsigned readings genuinely
disagree and the control column comes out opposite, which is what proves
the comparison is really signed rather than passing by accident. Checked
structurally (`ictus-frontend-verilog/tests/signed_compare.rs`, asserting
each composed shape, including that the unsigned control stays an
ordinary `Expr::Lt`, plus a negative test for the rejected mixed case)
and differentially against Icarus Verilog
(`ictus-cli/tests/differential_signed_compare.rs`).

**Array/memory signals**: done -- the one the roadmap had been flagging as
"distinct, likely-larger" since the very first bit-select work, and the
first increment since `localparam` that genuinely couldn't be done by
widening expression lowering. picorv32's own register file is the target
shape: `reg [31:0] cpuregs [0:regfile_size-1];`, written as
`cpuregs[latched_rd] <= ...` at a *runtime* index. Design in
decisions.md D22; the shape:

- `Signal` gains `depth: Option<u32>` -- `None` is an ordinary
  scalar/vector (everything that existed before, unchanged), `Some(n)` an
  array of `n` elements of `width` bits. `width` stays the *element*
  width, so nothing that already reasoned about it had to change.
- `Expr::ArrayRead { array, index }` and `Stmt::ArrayAssign { array,
  index, value }`. The write is a separate statement rather than another
  field on `NonBlockingAssign` because the two really are different
  writes -- one addresses an element chosen at simulation time and
  replaces it whole, the other a fixed signal and possibly just a
  constant bit range of it. That also makes "no bit range of an array
  element" structural rather than a runtime check.
- The kernel keeps array contents in a side table indexed by the same
  `SignalId` as the scalar values (empty `Vec` for a scalar, so lookups
  stay O(1) without a map), and `eval_expr` gained an `arrays` parameter.
  Deliberately *not* packed into the flat value buffer: that leaves
  scalar access -- the common case -- exactly as it was, and this
  interpreter's layout isn't the one that has to be fast (D12; the
  compiled kernel's bit-packed contiguous layout is architecture.md's
  concern, not this one's).
- An array index is evaluated in the same pre-edge snapshot as the
  right-hand side, so `mem[addr] <= x;` uses `addr`'s value from *before*
  the edge -- the pending write records the already-evaluated index, not
  the expression, precisely so it can't be re-read after earlier writes
  in the same tick have landed.
- Out-of-range: a read yields 0, a write is dropped. Same 2-state
  reasoning as `DynamicBitSelect`'s out-of-range bit index -- and
  dropping beats clamping or wrapping, which would corrupt a *different*
  element.
- Rejected rather than guessed at, each with its own test: an array as a
  module *port* (nothing in v1 can address it from outside), a
  bit-select of an array element (`mem[i][3]` -- unrepresentable on the
  write side, and allowing it only on reads would be a confusing
  asymmetry), an array declared alongside other names in one statement
  (the unpacked dimension is found by searching the declaration, which
  can't tell which name it belongs to), a continuous `assign` to an
  element, and any unpacked form other than `[high:low]`.

Verified structurally (`ictus-frontend-verilog/tests/array.rs`, including
that a *constant* index on an array is still an element read rather than
a bit-select -- the array check has to come first or `mem[2]` would read
bit 2) and differentially against Icarus Verilog
(`ictus-cli/tests/differential_array.rs`), whose sequence rewrites an
element while reading it (proving the read sees pre-edge contents) and
then reads a neighbour (so writing the wrong slot would show up). That
test fills the array in an *unsampled* warm-up phase first: a
never-written element reads as 4-state 'x' in Icarus but 0 here, so
sampling one would compare two different-but-both-valid answers rather
than test anything -- the same discipline D19 established. The
out-of-range read/write policies are covered by `ictus_kernel`'s own unit
tests for that same reason.

**Blocking assignment (`=` inside a clocked block) is done** -- the gap
the array work surfaced, and the one flagged above as needing a real
decision rather than a default, because it is not another statement form
but a second *write discipline*. Design in decisions.md D23.

In Verilog's own terms: `x <= value;` (non-blocking) schedules the write
so every read in that same block still sees the old `x` -- that is what
lets a shift register be written as a plain sequence of statements. `x =
value;` (blocking) writes immediately, so the next statement reads the
new value; it is how a signal gets used as a local variable mid-sequence.
picorv32 mixes both freely in one `always @(posedge clk)` block
(`set_mem_do_rinst = 1;` a few lines from `decoder_trigger <= 0;`), which
is what forced the issue. The shape:

- `Stmt::BlockingAssign { target, target_range, value }` and
  `Stmt::BlockingArrayAssign { array, index, value }` -- separate
  statements rather than a `blocking: bool` flag on the existing pair.
  The strict reading of the rule the earlier increments followed
  (separate variants for different *addressing*, flags for different
  *timing*) would have said flag; three things outweighed it, and D23
  records them: `Stmt::Assign` as a name collides with the existing
  continuous-`assign` struct, a match arm reading `BlockingAssign` states
  the timing where a reader needs it, and the duplication the rule exists
  to prevent -- duplicated *behaviour* -- is avoided by the shared write
  path below regardless.
- The kernel's evaluation phase can now mutate live state. `eval_stmts`
  takes `&mut` access to the value buffer and the array side table
  *alongside* the pending-write list; a blocking statement writes
  immediately, a non-blocking one queues as before, and the queue drains
  afterwards. That is the LRM's own active-region/NBA-region ordering
  rather than an approximation of it, and both of the cases that
  distinguish the two kinds fall out of it without special-casing:
  `a = 5; b <= a;` schedules `b` with 5, while `x <= 1; y = x;` gives `y`
  the *old* `x`.
- Both disciplines write through one extracted `apply_write`, so the
  partial-target read-modify-write (D14), the width masking and the
  out-of-range array rule (D22) exist once. The only thing that differs
  between blocking and non-blocking is *when* that function is called --
  which is exactly the distinction Verilog draws, and nothing else.
- Rejected with a test: compound assignment (`acc += in;`). sv-parser
  routes it through the same `OperatorAssignment` node as `=`, differing
  only in the operator symbol, so accepting the node blindly would treat
  `+=` as `=` and silently drop the accumulate.

Verified structurally (`ictus-frontend-verilog/tests/blocking.rs` --
that both kinds stay distinguishable through lowering, in source order,
since a blocking write is only correct *relative to* the statements
around it) and, load-bearingly, differentially against Icarus Verilog
(`ictus-cli/tests/differential_blocking.rs`): the difference between the
two kinds *is* the timing, which no structural check can show. That test
compares a blocking write read by the very next statement, a non-blocking
right-hand side reading a just-blocking-written signal, and -- the case
that fails if the two disciplines are collapsed into one -- a blocking
read of a signal a non-blocking write targeted two statements earlier,
which must still see the pre-edge value. Its first cycle is unsampled for
the usual 2-state/4-state reason (D19).

**Self-determined operand widths are done, and with them the whole of
picorv32.v lowers cleanly** -- 225 signals, no error. That is the first
time the real design has made it through the frontend end to end, and it
closes the diagnostic loop that has driven every increment since D13.
Design in decisions.md D24.

The gap itself turned out to be smaller than the previous entry claimed,
and *differently placed*: this was recorded here as a concatenation
problem because the error text said "concatenation operand's width can't
be determined", but `expr_width` has two callers and picorv32 tripped the
other one -- `|(irq_pending & ~irq_mask)` at picorv32.v:1538, a reduction
operator needing its operand's width to know how many bits to fold. The
message named only the caller it was first written for. It now describes
what the function is for instead, which is the transferable lesson: an
error raised by a shared helper shouldn't name one caller's context as
though it were the only one.

- `expr_width` (and `constant_expr_width`, kept in step) answers for
  `& | ^ + - *` as the wider of the two operands -- Verilog's
  self-determined width rule, not an approximation of it.
- For the bitwise three that is exact in the strongest sense: neither
  operand can set a bit above its own width, so nothing can be lost. For
  the arithmetic three the width is the same rule but the result can
  exceed it, and Verilog *discards the overflow* -- `{a + b, c}` with
  4-bit operands packs four bits, so the adder's carry is gone unless an
  operand is widened first (`{1'b0, a} + {1'b0, b}`).
- That trap was the argument for leaving arithmetic rejected, and it lost
  deliberately: refusing it here wouldn't prevent the trap, only move it,
  since the same truncation already happens whenever `a + b` is assigned
  to a narrower register -- which this frontend has always allowed.
  Matching Verilog uniformly, and naming the trap where the decision is
  made, beats an inconsistency the user has to discover.
- The truncation genuinely happens rather than being assumed: the kernel
  evaluates on `u64`, and every consumer of `expr_width` masks back down
  to it (concatenation packing, `BitwiseNot`, all three reductions).
- Shifts stay rejected, with a test. Verilog gives `a << b` its *left*
  operand's width -- a different rule, not the same change twice -- and
  folding them in on the assumption the rules match is exactly the guess
  this project declines to make.

Verified structurally (`ictus-frontend-verilog/tests/width.rs`) and
differentially against Icarus Verilog
(`ictus-cli/tests/differential_width.rs`), which pins the truncation
cases specifically: `a = 12, b = 10` makes `{a + b, 2'b11}` equal 27, not
the 91 an untruncated 5-bit sum would give.

**picorv32 now runs, and the answer to "does it simulate correctly?" is
"yes for the bus, not yet for execution"** -- a distinction the work
itself produced rather than one anticipated. Design and full findings in
decisions.md D25.

The harness is a *trace replay*, not a co-simulation. Ictus can't
instantiate modules, so it can't run a testbench; reimplementing the
memory model in Rust would have put two hand-written models on the two
sides of the comparison, where every disagreement between them looks
exactly like a simulator bug. Instead Icarus runs the testbench and
records both what it drove into the core and what the core produced, and
the Rust side replays the recorded inputs. The stimulus is then identical
by construction. Sampling is at each negedge, where nothing is moving, so
one row holds the outputs after edge N and the inputs for edge N+1
together -- and replaying a row takes two steps, since a combinational
output reacts to an input with no edge in between.

Three real defects came out of it, and the common thread is the point:
each let the design lower cleanly, run without error, and produce
plausible output while being wrong. None was reachable by reading the
code, and none would have been caught by a fixture-sized test.

- **`Simulation::set` didn't re-settle combinational logic**, so a `get`
  between ticks reflected the *previous* inputs. picorv32's
  `assign mem_xfer = mem_valid && mem_ready;` reacts to an input with no
  edge in between, so the whole memory handshake lagged a cycle. `set`
  now settles rather than exposing a `settle()` the caller must remember
  -- forgetting that would be silent.
- **Net declarations with an initializer were silently dropped.**
  `wire mem_done = ...;` *is* a continuous assignment, and picorv32 uses
  that spelling for most of its combinational logic (59 of them against
  43 standalone `assign` statements). The wires existed and read 0
  forever. A *variable* initializer (`reg x = 0;`) looks identical and
  means something else entirely -- once, before time zero -- and is now
  rejected rather than mis-lowered or ignored.
- **Binary operator precedence was never applied at all.** sv-parser
  returns a right-leaning chain in source order, so
  `a == b && c == d` arrived as `a == (b && (c == d))`. picorv32's
  instruction decoder is built from exactly this shape, so the core
  decoded `addi` as a shift instruction and kept running.
  `lower_binary_chain` now re-associates by real precedence, which
  subsumes D13's narrow ternary fix (a bare `?:` binds looser than every
  binary operator, so it can only end a chain) and also fixed an
  unnoticed associativity bug -- `a - b - c` was `a - (b - c)`.

After all three, every traced port matches Icarus exactly, every cycle.
**And picorv32's register file in Ictus is still empty.** The core
reproduces the bus trace while executing nothing, because `cpuregs_write`
and `cpuregs_wrdata` come from an `always @*` block this frontend doesn't
lower; for straight-line code the fetch addresses don't depend on any
register value, so a dead datapath is invisible from the ports. Agreement
on a design's ports is not evidence that the design ran, and the test now
says so in its own comments -- it asserts the register file is *still*
empty, so that assertion fails loudly the moment the gap closes.

**Combinational `always` blocks are done, and with them picorv32 actually
executes.** The bus trace already matched; now the register file matches
Icarus too, across a program doing ALU work, a store, a load and a
branch. Design in decisions.md D26.

- `always @*`, `always @(*)` and `always_comb` lower to a new
  `ictus_ir::CombProcess`. No sensitivity list is stored -- all three
  mean "re-run on whatever the body reads", so recording one would only
  create a second source of truth. An *explicit* list (`always @(a or
  b)`) is rejected rather than widened to `@*`: an incomplete list is a
  classic Verilog bug, a conforming simulator honours it as written, and
  widening it would disagree with the reference exactly where it matters.
- The body needed no new statement machinery. These blocks are
  `if`/`case` plus **blocking** assignment, which the earlier increment
  (D23) turns out to have been the prerequisite for -- "a later statement
  sees what an earlier one wrote" *is* the execution model here. A
  non-blocking assignment inside one is rejected: it defers past the
  block's own later statements, and settling has no deferral phase.
- The kernel now settles to a **fixpoint** -- all continuous assignments
  and all combinational blocks, re-run until a whole pass changes nothing
  -- replacing the single declaration-order pass flagged as a limitation
  above. Order-dependence is removed rather than documented, which is what
  makes these blocks safe: two of them can feed each other in either
  direction and no source order is right for every design.
- **The subtle part, got wrong first**: convergence has to compare state
  before and after a whole pass, not ask each write whether it changed
  anything. Combinational Verilog's dominant idiom is assign-a-default-
  then-override, so every pass writes twice and can end where it began; a
  per-write flag is true forever. That first version reported picorv32 as
  having a combinational loop.
- Logic that never converges panics, naming the signals still moving. A
  conventional simulator answers a combinational loop with a hang or an
  oscillating waveform; saying which signals oscillate beats both.
- Inferred latches fall out and are not special-cased: a block that
  doesn't assign on every path leaves the previous value in place, which
  is what Verilog means by one. Flagging it was considered and rejected --
  this is a simulator, and a latch is legal, unambiguous Verilog.
- A latch did force one API addition, `Simulation::set_all`, which drives
  a group of inputs *in one instant*. For ordinary combinational logic
  that's indistinguishable from repeated `set` calls; for a latch it is
  not, since lowering an enable after changing the data lets the latch
  capture it first. Verilog draws the same distinction by whether time
  advanced between the assignments. This showed up as a real
  Icarus/Ictus disagreement, not as a theory.
- `negedge`, `always_ff` and `always_latch` blocks are now **rejected**
  rather than silently skipped. Everything that isn't `posedge` used to
  be ignored, which is what hid `always @*` in the first place.
- String literals landed alongside, because reaching those blocks reached
  picorv32's disassembly signal (`new_ascii_instr = "lui";`). IEEE 1800
  §5.9 makes a string literal in an expression its characters packed 8
  bits each, so `"lui"` is a 24-bit `0x6C7569`; capped at 8 characters,
  which is exactly what fits this kernel's `u64` values and exactly
  picorv32's longest.

Verified structurally (`ictus-frontend-verilog/tests/comb_always.rs`,
including all three rejections), differentially against Icarus
(`ictus-cli/tests/differential_comb_always.rs`, covering
default-then-override, a block that reads one declared after it, and a
latch observed across a change of its data input), by `ictus_kernel`'s
own unit tests for the two things no reference simulator can answer
(order-independent settling, and a combinational loop being reported
rather than hanging), and end-to-end by the picorv32 test, which now
compares the register file.

**`generate if` is now elaborated -- and picorv32 had been passing partly
by coincidence until it was.** Design in decisions.md D27.

The frontend walked each module with sv-parser's deep iterator, which
yields the contents of *both* branches of a `generate if`, so both were
lowered. picorv32's `generate if (TWO_CYCLE_ALU)` produced a clocked ALU
and a combinational ALU driving the same signals, correct only because
settling ran the combinational one last; its `ENABLE_MUL`/`ENABLE_DIV`
branches hold module instantiations, which were silently dropped, leaving
the `else` tie-offs that happen to suit the default parameters. Nothing
caught it -- it surfaced when starting on the firmware below raised the
question of how parameters select code at all.

- `generate if` conditions are folded to constants against the resolved
  parameters, and the source spans of unselected branches are recorded.
  Every walk in the frontend skips items inside one -- a filter rather
  than a pruned tree, because each walk iterates independently and the
  spans reuse offsets the tree already carries. `else if` needs nothing
  extra; a construct inside an excluded branch is skipped, never
  evaluated, so an unselected branch can contain anything.
- Rejected where they occur in code that exists, rather than skipped:
  **module instantiation** (previously dropped silently -- picorv32 with
  `ENABLE_MUL=1` would have lowered with no multiplier and no error),
  **`generate for`/`generate case`**, and **a parameter declared inside
  any `generate if`** (parameters resolve before branches are chosen, so
  the both-branches idiom would silently take the last value).
- **One driver per signal**: a signal written by more than one process or
  continuous assignment is rejected. That is the invariant the bug broke,
  and it would have refused the old lowering outright. Stricter than
  Verilog on purpose -- two net drivers need a resolution rule a 2-state
  kernel lacks, and two blocks writing one variable race -- though it does
  turn away disjoint-bit-range writes from separate blocks, which are
  legal and well-defined.

Verified by breaking it: with elaboration disabled, the differential test
(`ictus-cli/tests/differential_generate.rs`) fails only on its
*between-edge* samples. Every post-edge sample still matches Icarus,
because right after an edge a registered and a combinational `a + b`
agree -- so a test sampling only after edges would have passed the bug.

**Next: picorv32's own instruction tests.** A RISC-V cross-compiler is
available here (`riscv64-unknown-elf-gcc`, targeting rv32 via
`-march`/`-mabi`), so the per-instruction tests in
`bench/designs/picorv32/tests/` can be assembled for picorv32's
*default* configuration -- base ISA only, no multiply, divide, interrupts
or compressed instructions -- and run through the existing trace harness
by loading the image into the testbench memory with `$readmemh`. That is
the longer, more demanding program the roadmap called for, and it no
longer waits on anything else. The prebuilt `firmware.hex` does not fit
yet: it was built for `COMPRESSED_ISA`, `ENABLE_MUL`, `ENABLE_DIV` and
`ENABLE_IRQ`, and the multiply and divide units are separate modules, so
it needs both top-level parameter overrides and module instantiation.

After that, the larger pieces: **module instantiation** (the biggest
remaining language gap, and what would let Ictus run a testbench
directly), and **Cranelift codegen**, now that D12's precondition -- a
correctness baseline with a real design behind it -- is met.

The supported language subset is still intentionally narrow: single
ANSI-style module, any number of clocked processes, combinational
`always @*`/`always_comb` blocks, and `assign`s (in both spellings -- a
standalone `assign` and a net declaration carrying an initializer),
`#(parameter ...)` *and* `localparam` sharing one
resolution pass (value expressions may reference an earlier parameter/
localparam, and use `+ - * << >> >>> & | ^ == != < <= > >= && ||`, the
ternary operator, and concatenation; no overriding a `parameter` at
instantiation), constant/variable bit-select and constant part-select,
concatenation (plain and replication) and ternary on reads only -- with
comparison/logical/reduction results and binary bitwise/arithmetic
results, not just literals/refs/selects, valid as concatenation operands
(each at Verilog's self-determined width, so an arithmetic carry is
truncated away), but *not* a shift result -- a constant
bit-select/part-select --
or a concatenation of such -- as a procedural assignment target but not a
continuous (`assign`) one and not with a variable index (though the
index/bound *may* reference a parameter/localparam, per the
constant-folding work), `$signed(...)` to sign-extend a value into a
wider assignment target and to mark operands for the signed-aware
operators -- a real arithmetic right shift (`$signed(x) >>> n`) and a
real signed ordering comparison (`$signed(a) < $signed(b)`) -- but *not*
a logical right shift of a `$signed(...)` value, nor a *mixed*
signed/unsigned comparison (and no other system function), a call to a
provably-empty task but no other task/function calls, logical `!`,
bitwise `~`, and the reduction operators, a 4-state `x`/`z` literal
outside a case item (resolves to `0`), array/memory signals (`reg [31:0]
mem [0:31]`) with one unpacked dimension, internal-only, one element at a
time -- but no array port, no bit-select of an element, and no `assign`
to one -- and both `<=` and `=` inside a clocked block, but no compound
assignment (`+=` and friends), no explicit sensitivity list, no `negedge`
block, `generate if` but not `generate for`/`generate case`, no module
instantiation, and at most one driving process or assignment per signal.
The whole of picorv32.v lowers within
this subset and both its bus behaviour and its register file match Icarus
while it executes a program; Cranelift codegen is further out still.


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
