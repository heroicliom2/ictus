# Decisions

Lightweight ADR (Architecture Decision Record — a standard short format for
recording one technical decision, its alternatives, and its reasoning; see
glossary.md) log. Each entry: the decision, the alternatives considered,
and why — so a fresh agent doesn't re-litigate settled questions or redo
rejected work. Newest at the bottom. Add an entry whenever a real fork in
the road gets resolved in conversation; don't bother for routine
implementation choices that don't foreclose an alternative.

*Software/compiler-engineering terms below (JIT, Cranelift, dataflow
graph, static/dynamic scheduling, SIMD, software licenses, etc.) get a
short inline gloss on first use; full explanations are in
[glossary.md](glossary.md).*

## D1 — Strategy: performance wedge, not feature parity

**Decision**: target beating Verilator on RTL simulation speed and
edit-simulate turnaround. Do not target QuestaSim/Vivado feature parity
(UVM, SVA, coverage, gate-level/SDF, IP encryption) in the near term.

**Alternatives considered**: open/hackable ecosystem wedge (pluggable IR,
scriptable, modern toolchain integration where commercial tools are closed);
cost/accessibility wedge (free/cheap, "good enough" for the segment priced
out of $50k-500k/seat licenses); full parity as a long-term funded/team
effort.

**Why**: full parity is a decade-plus, large-team undertaking regardless of
which of these paths is chosen — that's just the size of what Questa/Vivado
actually are. "Faster, yes/no" is a provable, measurable claim; "as complete
as Questa" never resolves. Verilator itself won real adoption without
verification-completeness parity, by being dramatically faster and free.
Performance is also the option with the clearest near-term win condition
given the other paths all still require the performance work as a
foundation anyway.

**Superseded by D11** — this entry's "beat Verilator" framing was too
narrow and is corrected there. The underlying technical decisions this
entry justifies (cycle-based kernel, JIT, static partitioning, 2-state
default — D2/D3/D5/D6) all still stand; what changed is *why* they matter
(they serve pillar 1 of three, not the whole strategy).

## D2 — Kernel: cycle-based primary engine + secondary event-driven testbench layer

**Decision**: primary execution engine topologically evaluates the design
once per clock edge (cycle-based), not a classic delta-cycle event queue. A
smaller, separate event-driven kernel handles non-synthesizable testbench
code alongside it.

**Alternatives considered**: single classic event-driven kernel with delta
cycles and active/inactive/postponed queues for everything (the original
starting proposal).

**Why**: cycle-based evaluation is what makes Verilator fast; a fine-grained
event queue as the *primary* engine is the interpreted-simulator model and
structurally can't hit the same throughput. The trade is giving up exact
sub-cycle delta-cycle fidelity for synchronous RTL, which is acceptable
given the performance-wedge strategy (D1). Pure cycle-based alone can't
correctly run arbitrary testbench code (`#delay`, `fork`/`join`), hence the
second tier rather than picking one kernel model for both jobs.

## D3 — Execution: Cranelift JIT, not AOT-via-system-compiler

**Decision**: compile the elaborated design to native machine code
in-process via Cranelift at load time ("JIT," Just-In-Time — compiling
code to run right when it's needed, inside the same running program),
rather than generating C++/Rust source and shelling out to a separate
system compiler as its own build step ("AOT," Ahead-Of-Time — Verilator's
model).

**Alternatives considered**: generate C++ and invoke GCC/Clang (copy
Verilator's approach); generate Rust and invoke `rustc`.

**Why**: for large designs, Verilator's separate AOT recompile step can
take minutes and dominates the actual edit-simulate loop — this is
arguably Verilator's real adoption friction, more than raw runtime
throughput. Cranelift (a Rust-native code generator — a component that
turns a lower-level program representation into actual CPU instructions;
it's the JIT backend behind the Wasmtime WebAssembly runtime) gives fast
codegen and fast turnaround without a second toolchain dependency. An
optional AOT path (emit to `rustc`/LLVM, a larger and more established
compiler backend that produces more optimized but slower-to-generate code)
is kept as a phase-6 stretch goal for max-throughput CI runs where compile
time matters less — mirrors `cargo run` vs `cargo run --release`.

## D4 — Native SystemVerilog testbench execution as a differentiator

**Decision**: support running plain SV testbenches directly (interpreted,
for the non-synthesizable layer), not require a C++/SystemC harness.

**Why**: this is Verilator's biggest practical adoption barrier — you
cannot point it at a normal SV testbench without rewriting the bench in
C++/SystemC. Closing this gap is a real workflow win independent of the
speed pitch, and is enabled directly by the two-tier kernel design (D2).

## D5 — Parallelism: static partitioning, not fine-grained per-process concurrency

**Decision**: partition the dataflow graph into independent clusters once,
at elaboration time, and assign to threads with a static schedule
(Verilator's MTask approach).

**Alternatives considered**: work-stealing (a dynamic-scheduling technique
where idle threads grab tasks from busy threads' queues at runtime, rather
than each thread having a fixed, pre-decided job) across independent
processes at runtime, using the Rust `rayon` library — part of the
original starting proposal.

**Why**: fine-grained parallel event-driven RTL simulation is a known-hard
research problem — event granularity is typically too fine (too many, too
small individual pieces of work) for thread-per-process to pay off once
synchronization overhead and shared-signal fan-out dependencies are
accounted for. Static partitioning (deciding once, in advance, which
thread handles which part of the design, with no runtime decision-making
overhead) is the proven version of the same intuition and is what
Verilator actually ships.

## D6 — Signal representation: 2-state default, 4-state opt-in

**Decision**: bit-packed (many individual signal bits stored tightly
together inside larger machine words rather than one byte/object per bit)
and SIMD-friendly (laid out so the CPU can process many signal bits with
one instruction — Single Instruction, Multiple Data) 2-state vectors by
default. 4-state (X/Z) tracking is opt-in per-signal or per-module.

**Why**: directly follows Verilator's own hard-won performance lesson —
4-state tracking as the default representation is expensive and mostly
unneeded once RTL is verified. Naive per-bit enums were rejected outright;
this was never a real alternative given the performance-wedge strategy.

## D7 — Waveform writer: do not port gtkwave's C core

**Decision**: use `wellen` (BSD-3) for waveform reading (same library
Surfer itself uses). If FST writing isn't covered by wellen, write a
clean-room encoder against the public FST format spec, or base it on
`libfstwriter` (confirmed MIT) — never port code from gtkwave's own
`fstapi.c`/libfst.

**Alternatives considered**: port/adapt gtkwave's existing FST writer
(the original starting proposal, since gtkwave is the reference
implementation and has "prior art").

**Why**: gtkwave itself is GPLv2 — a "copyleft" license meaning code built
using GPL sources generally has to be released under the GPL too, so
pulling GPL code into a project generally forces the whole project's
license to change (see glossary.md, Licensing terms). There's also open,
unresolved ambiguity in gtkwave's own issue tracker (gtkwave/gtkwave#309)
about whether `fstapi.c`/libfst specifically is GPL or MIT (MIT is
"permissive" — no such license-change risk), plus an optional LGPL-gated
(a weaker, partial copyleft) Judy-array code path. Porting from gtkwave's
core risks pulling GPL-encumbered code into what should stay a
permissively-licensed (MIT/Apache-2.0) project. `wellen` (BSD-3-licensed,
also permissive) sidesteps this entirely for reading and is already the
dependency Surfer integration (architecture.md, Debug UI) needs anyway.

## D8 — VHDL frontend: `vhdl_lang` is actively maintained, not stale

**Decision**: treat `vhdl_lang` (VHDL-LS project) as a healthy dependency
for the phase 4 VHDL frontend, not a project requiring the "couple years
stale" rescue effort originally assumed.

**Why**: verified the project is maintained under the VHDL-LS GitHub org
(not just original author kraigher solo), with a real analyzer core
(`vhdl_lang`) separate from its LSP layer (`vhdl_ls`), and is fast (200k-line
project analyzed in ~160ms). The original "stale, may need to extend"
framing was an unverified assumption that didn't hold up on checking.

## D9 — Project name: Ictus

**Decision**: name the project **Ictus**.

**Alternatives considered and rejected**:
- **Smelt** — direct collision: `silogy-io/smelt` already exists as a Rust
  chip-verification test runner. Same industry, adjacent purpose, too close
  to risk confusion.
- **Ferrox** — namespace too crowded: multiple unrelated Rust projects use
  it, including a full branded "Ferrox" enterprise framework ecosystem
  (ferrox-rust.dev).
- **Wafer** — no collision, reads instantly as semiconductor-related, but
  it's a generic English word (also a cookie) that doesn't carry any of the
  "fast/compiled" positioning.
- **Rustle** — low collision risk (a few small unrelated crates: a Svelte
  compiler clone, a download manager, a Wordle clone; nothing in EDA), fun
  Rust-ecosystem pun, but the hardware connection only lands once explained.

**Why Ictus**: Latin/musicology term for the stressed beat in a rhythm —
directly names the actual technical bet (a cycle/beat-based kernel that
evaluates the design once per clock edge, D2), not just "hardware" or
"Rust" generically. Short, pronounceable, CLI-friendly (`ictus build`,
`ictus run`), and clean in the namespace as of the 2026-09-10 check.

## D10 — Scope boundary: verification features stay out unless the wedge succeeds

**Decision**: UVM, constrained-random, functional/code coverage, full SVA,
gate-level timing/SDF, IP encryption (P1735) are explicitly not planned
work, not just "later" — they are not on the roadmap at all right now.

**Why**: each would roughly 10x project scope without moving the "is it
faster" needle that the entire strategy (D1) is built around. Revisit only
if the performance niche actually gets traction and there's a deliberate
reason to expand up-market — don't let scope creep back in by default.

**Superseded by D11** — full verification-suite support is a real long-term
goal, not a permanently rejected one. The near-term phase scope (roadmap.md)
is unchanged; what changed is the reason these are deferred rather than
ruled out.

## D11 — Strategic reframe: three pillars, not a "beat Verilator" wedge

**Decision**: the project's actual identity rests on three pillars that
every design choice gets checked against, and none of them is disposable
in favor of another:

1. **Speed / lightweight execution** — the cycle-based kernel, Cranelift
   JIT, static partitioning, 2-state-default work from D2/D3/D5/D6 all
   stand as-is.
2. **True mixed-language support, natively** — Verilog/SystemVerilog and
   VHDL in the same simulation, with cross-language elaboration and port
   binding as a first-class concern, and critically: **no requirement to
   drop into C/C++ or SystemC models** to make any of this work (unlike
   Verilator, which requires a C++/SystemC harness for the testbench
   layer). This is not a "nice to have" alongside speed — it is co-equal
   to it.
3. **A genuine long-term path to full verification-suite capability**
   (UVM-class methodology, SVA, coverage) as an eventual, real goal — not
   permanently excluded scope. It is *sequenced* behind pillars 1 and 2,
   added only when and how it can be done without compromising them, never
   dropped as an ambition.

**Revises**: D1 (which framed the whole project as a "performance wedge,
beat Verilator" strategy) and D10 (which framed verification features as
simply "not on the roadmap," full stop). The near-term phased roadmap
(phases 0-5) does not change as a result of this entry — Verilog-first,
RTL-first, no UVM yet is still correct sequencing. What changes is *why*:
this was never really about winning a speed benchmark against Verilator: it
was about there being no real open-source simulator capable of
mixed-language (VHDL + Verilog/SV) RTL simulation at all, without forcing
users into C/C++ models. That gap is the actual reason this project exists.
Speed and "lightweight" stay as hard constraints (a slow or bloated tool
fails pillar 1 regardless of how complete it is), but they are not the
headline pitch — filling the mixed-language gap is.

**Why**: stated directly by the project owner — the original motivation for
starting this project was observing that no open-source tool does
mixed-language RTL simulation well, and the goal is for Ictus to eventually
be a credible open-source replacement for people currently locked into
QuestaSim-class proprietary tools, without ever regressing on speed or
forcing a C/C++ dependency to get there. Framing this as "beating Verilator"
was too narrow and implicitly deprioritized the mixed-language and
eventual-completeness goals — this entry corrects that.

**Practical implication for future decisions**: when evaluating any new
feature or architectural choice, check it against all three pillars, not
just whichever one is most top-of-mind at the time. A feature that helps
verification-completeness but meaningfully hurts speed, bloats the binary,
or requires a C/C++ shim to implement mixed-language support needs an
explicit tradeoff discussion, not a default yes.

## D12 — Phase 1 implementation order: interpreter before Cranelift JIT

**Decision**: `ictus-kernel`'s first working version is a plain tree-walking
interpreter over `ictus-ir` (see that crate's module doc comment), not the
Cranelift-JIT-compiled engine D3/architecture.md describe as the target.
The interpreter's external behavior (a `Simulation` with `set`/`get`/`tick`)
is meant to be what a later JIT-based implementation still honors --
replacing internals, not the validated semantics.

**Why**: D3 fixed the *target* execution model (Cranelift JIT), but not the
*build order*. Writing JIT codegen for `if`/non-blocking-assignment/expression
evaluation before anything has ever been checked against a reference
simulator would mean optimizing correctness-unknown code -- there'd be
nothing to tell you whether a wrong answer came from the frontend's
lowering, the semantics, or the codegen itself. The interpreter is cheap
to write and to get right, and it's what phase 1's actual acceptance bar
(docs/roadmap.md: differential match against a reference simulator) is
checked against here. Concretely: `crates/ictus-frontend-verilog/tests/counter.rs`
and `crates/ictus-cli/tests/differential_counter.rs` lower and run a small
clocked counter design and check the result against Icarus Verilog,
cycle-for-cycle, via this interpreter.

**Practical implication**: don't read `ictus-kernel`'s current
interpreter as the finished kernel architecture -- it's the correctness
baseline the eventual JIT engine gets built and checked against, per this
entry, not a change to D2/D3's target design.

## D13 — Work around a real `sv-parser` precedence bug for `binop ? :`

**Decision**: `ictus-frontend-verilog::lower_expr`'s `E::Binary` arm
detects and corrects a specific `sv-parser` mis-parse: an *unparenthesized*
ternary operator immediately following a binary operator's right operand
(`a > c ? a : c`) comes back from `sv-parser` structured as if it were
`a > (c ? a : c)` -- a `Binary` node whose right-hand side is itself a
bare `ConditionalExpression` -- rather than the only semantically-correct
reading, `(a > c) ? a : c`. Every binary operator this frontend lowers
binds tighter than `?:` in real Verilog, so this input is unambiguous;
`sv-parser` gets it wrong. The fix rewrites the mis-nested tree: pull the
inner ternary's `cond` out, re-apply the outer binary operator to just
that (`apply_binary_op(op, lhs, inner.cond)`), and use the inner
ternary's `then`/`else` as the new outer ternary's -- see the arm's own
comment for the exact transform and its known limit (only the *immediate*
right-operand case is fixed; a ternary buried deeper on the right, e.g.
past a nested binary operator, is corrected at that inner level during
recursion but an outer operator needing to *also* re-associate past an
already-fixed inner ternary is not handled).

**Alternatives considered**: reject unparenthesized `binop ? :` outright
and require source changes -- rejected immediately, since the whole point
of lowering real designs (picorv32) is running the source as written, not
demanding it be rewritten; requiring every Verilog file this project
might ever lower to be hand-edited first defeats the purpose. Patching
`sv-parser` itself upstream -- plausible, worth doing eventually (it's a
real bug, not just a limitation, and other consumers would hit it too),
but out of scope for unblocking this project right now, and the local fix
is small, well-contained, and easy to test in isolation regardless of
whether an upstream fix ever lands.

**Why discovered / confirmed, not assumed**: found by actually lowering
`bench/designs/picorv32/picorv32.v` (a real design already vendored for
phase 0) and inspecting the exact mis-lowered tree for
`docs/roadmap.md`-worthy diagnostic value; confirmed as specifically a
missing-parens issue (not a general ternary bug) by writing the identical
expression both with and without explicit parens around the condition and
comparing the two lowered trees -- see
`ictus-frontend-verilog/tests/ternary_precedence.rs` and
`ictus-cli/tests/differential_ternary_precedence.rs`, the latter checked
against Icarus Verilog including a case where the condition is false, not
just the "obviously worked" case.

**Practical implication for future work**: if a *new* binary operator is
added later (subtraction, shift, multiply, ...), it automatically goes
through the same `E::Binary` arm and gets this fix for free -- no
per-operator repetition needed. If a similar mis-association is ever
found for a *different* operator pair (e.g. unary operators, once/if
their operand type ever changes from the current `Primary` restriction
that structurally prevents this class of bug today), treat it as its own
new finding to verify empirically the same way, not an assumed
extension of this one.

## D14 — Partial-write commit: direct sequential mutation, not a staged map

**Decision**: a non-blocking assignment to a constant bit-select/
part-select target (`x[7:0] <= v;`, `ictus_ir::Stmt::NonBlockingAssign`'s
new `target_range: Option<(u32, u32)>` field) is committed in
`ictus_kernel::Simulation::tick()` as a read-modify-write directly against
the live `values` array, applied in the same order the pending writes were
collected in -- *not* staged into a separate map/buffer and merged in
afterward. This matters when more than one non-blocking assignment in the
same tick targets the same signal (picorv32 does this: disjoint field
writes to one register in a single `always` block, e.g. one statement
setting `mem_rdata_q[14:12]` and another setting `mem_rdata_q[31:20]` in
the same edge) -- direct sequential mutation lets a later partial write
correctly build on top of an earlier one from the same tick, matching real
hardware's "these are separate always-block statements, later one wins for
any bits it touches" behavior for same-signal, disjoint (or overlapping)
partial writes.

**Why this doesn't break the non-blocking-read rule**: Verilog's `<=`
semantics require every right-hand side in a clock edge to see the state
as it was *before* the edge, regardless of statement order -- normally the
reason a "stage all writes, then apply" two-phase design is necessary at
all. That rule is preserved here because the *evaluation* phase
(`eval_stmts`, which reads `self.values` to compute every RHS) already
runs to completion, producing a `Vec` of pending `(SignalId,
Option<(u32,u32)>, u64)` updates, before the commit loop below it ever
touches `self.values`. The commit loop's sequential mutation only matters
for how multiple writes *to the same signal* interact with each other
after evaluation is done -- it never feeds back into any RHS evaluation,
because evaluation is already finished by the time it runs. A staged-map
approach would need extra logic (merge-by-signal, applied in original
order) to get the identical result; direct sequential mutation gets it for
free from the loop's natural ordering.

**Alternatives considered**: stage partial writes into a `HashMap<SignalId,
u64>` (starting from each touched signal's pre-tick value) and merge bit
ranges into it before a single final write per signal -- rejected as
unnecessary complexity once it was clear direct mutation produces the
identical result (see above), for the cost of an extra data structure and
a less obvious "why is this correct" argument than "evaluation finishes
before commit starts, so commit-time order is free to matter." Making a
same-signal multiple-partial-write conflict an error instead of
"last write's bits win" -- rejected because it's legal, common Verilog
(picorv32 relies on it) with well-defined real-hardware semantics; erroring
on legal source would violate this project's own "never silently
mis-lower, but don't reject what real Verilog allows either" stance.

**Scope boundary drawn at the same time**: only `Stmt::NonBlockingAssign`
(`<=`) carries `target_range` -- `ictus_ir::Assign` (continuous `assign`)
deliberately does not. Continuous assignment has no commit phase at all
(`settle_combinational` is a single pass that evaluates and writes each
`assign` immediately, every call), so there's no natural place to do a
read-modify-write without reworking that pass's single-replace design;
`assign x[7:0] = v;` is still rejected by the frontend rather than
half-supported. Likewise, only a *constant* bit-select/part-select target
is supported -- a variable-indexed target (`x[i] <= v;`) or indexed-range
target (`x[base +: width] <= v;`) is rejected, because the write range has
to be known at lowering time to live in the IR as a plain `(u32, u32)`;
supporting either would need the kernel to carry and evaluate an index
expression at commit time instead, a different (larger) design not yet
needed by any real gap found so far.

**Why discovered / confirmed, not assumed**: found the same way as D13 --
lowering `bench/designs/picorv32/picorv32.v` and grepping its actual
`mem_rdata_q[...]` usage, which showed both the common case (single
constant part-select target per statement) and the specific
multiple-partial-writes-per-tick pattern this decision is about. Verified
with a dedicated fixture (`select_target_test.v`, two disjoint partial
writes to one register in the same edge, one of them wrapping at its own
narrower width) checked cycle-for-cycle against Icarus Verilog
(`ictus-cli/tests/differential_select_target.rs`), plus `ictus_kernel`
unit tests isolating the same-tick-combination and per-range-masking
behavior from the frontend entirely.
