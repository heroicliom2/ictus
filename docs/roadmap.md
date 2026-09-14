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
  `ictus-frontend-verilog/tests/select.rs` (which, like the `casez` test,
  also confirms a bit-select used as an *assignment target*
  (`result[3:0] <= v;`) is actively rejected rather than silently lowered
  as a full-width write -- v1's `Signal` model has no notion of a partial
  write, so silently dropping the select would write the wrong bits with
  no error) and `ictus-cli/tests/differential_select.rs`.

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

**Next confirmed blocker**: bit-select/part-select as an *assignment
target* (`mem_rdata_q[...] <= ...` -- picorv32 does this). Meaningfully
bigger than the read-side support that already exists: a partial-width
write needs read-modify-write semantics in the kernel (write just the
selected bits, leave the rest of the signal's current value alone), not
just frontend parsing -- currently `reject_select_target` explicitly
rejects this rather than silently truncating to a full-width write. Not
yet attempted.

The supported language subset is still intentionally narrow: single
ANSI-style module, any number of clocked processes and `assign`s but no
`always_comb`, module parameters (defaults must be constant literals, no
overriding at instantiation), constant/variable bit-select, constant
part-select, concatenation, and ternary on reads only (no indexed
part-select, not as a write target), no array/memory signals (`reg
[31:0] mem [0:31]` -- this is what picorv32's register file actually
needs, and is a distinct, likely-larger gap from bit-select on a single
signal), no module instantiation. Cranelift codegen and actually getting
picorv32 fully through the pipeline are both still ahead of where this
stands today.

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
