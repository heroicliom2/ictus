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

The supported language subset is still intentionally narrow: single
ANSI-style module, any number of clocked processes and `assign`s but no
`always_comb`, no `else if`, plain `case` only (not `casez`/`casex` --
those need wildcard-bit-aware literal parsing and comparison this IR
doesn't represent yet), constant bit-select/part-select on reads only (not
`x[i]`, not as a write target, not concatenation `{a,b}`), no module
instantiation. Cranelift codegen, phase 0's actual benchmark designs
(picorv32 first), and the gaps above are all still ahead of where this
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
