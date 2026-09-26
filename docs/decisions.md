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

## D15 — Concatenation assignment target: split at lowering time, no new IR

**Decision**: `{a, b[3:0]} <= value;` (a concatenation used as a
non-blocking assignment target -- picorv32's own style:
`{mem_rdata_q[31:25], mem_rdata_q[11:7]} <= {...};`) is lowered by
splitting it into several plain `ictus_ir::Stmt::NonBlockingAssign`s, one
per part, entirely inside `ictus-frontend-verilog::lower_concat_target_assign`
-- not by adding a new `Stmt` variant that carries multiple targets. Each
part's value expression is the (lowered-once) right-hand side, cloned and
wrapped in an `Expr::Select` picking out that part's bit range -- also not
new IR, since `Select` already exists for the read side. `ictus_ir` and
`ictus_kernel` are completely unchanged by this feature: as far as either
is concerned, `{carry, acc[7:4], acc[3:0]} <= v;` is indistinguishable
from three hand-written statements `carry <= v[8:8]; acc[7:4] <= v[7:4];
acc[3:0] <= v[3:0];` in the same `always` block (with `v` duplicated by
reference into three separate expression trees, one per statement -- each
gets its own clone, not a shared mutable node).

**Why this is correct despite evaluating the right-hand side more than
once**: unlike a hardware description with side-effecting reads (there
are none in synthesizable RTL) or a software expression with function
calls, `value`'s clones are pure -- re-evaluating the identical expression
tree against the identical pre-tick snapshot several times in the same
tick always produces the identical result each time. There is no risk of
the clones disagreeing with each other or with a hypothetical
single-evaluation version.

**Alternatives considered**: a new `Stmt::ConcatAssign { parts:
Vec<(SignalId, Option<(u32,u32)>)>, value: Expr }` IR variant that the
kernel would split internally at commit time -- rejected as pure
duplication of logic that already exists in two places (the
value-slicing math is identical to what a single select target's
`Expr::Select` already does; the multi-target application is identical to
what `tick()`'s per-signal commit loop already does for two ordinary,
unrelated `NonBlockingAssign`s in the same tick) for no behavioral
benefit -- splitting at lowering time gets the same result while keeping
the kernel's mental model exactly as simple as it was before this
feature: "a list of independent partial or full-width writes to apply in
order," never anything that needs to reason about a single right-hand
side fanning out to several targets at once.

**Scope boundary drawn at the same time**: a **nested** concatenation
inside the target (`{a, {b, c}} <= v;`) is rejected outright, not
supported by recursing this same splitting logic -- picorv32 doesn't use
this style anywhere, so there was no real case to verify against, and
"reject and wait for a real need" matches this project's standing
practice ([[ictus-development-practice]]) of not guessing at unverified
gaps. Only `<=` (non-blocking assignment) supports a concatenation
target; `assign {a,b} = v;` (continuous) is still rejected for the same
reason a single select is (D14's scope-boundary paragraph): `ictus_ir::Assign`
has no commit phase to split a value across multiple targets in.

**Why discovered / confirmed, not assumed**: found the same way as D13
and D14 -- after D14 (single select as a non-blocking target) landed,
re-running the picorv32 lowering diagnostic surfaced this as the very
next blocker, with real usage sites at picorv32.v lines 449, 496, 502,
and 537 grepped and read directly rather than guessed at (which showed
both a same-signal-multiple-selects case and, on the same lines, a plain
whole-signal part mixed in). Verified with two fixtures: a minimal one
matching the exact shape the pre-existing
silently-writes-only-the-first-part bug (fixed earlier this phase, before
concatenation targets were rejected at all) would have hit, and one
mirroring picorv32's own style directly (selects of the same signal mixed
with a plain signal, right-hand side itself a concatenation) -- the
latter checked cycle-for-cycle against Icarus Verilog via a nibble-swap
design chosen specifically so the expected output is easy to hand-verify
(`ictus-cli/tests/differential_concat_target_select.rs`).

## D16 — `$signed(...)`: sign-extend-to-64-bits marker, not general signed types

**Decision**: `$signed(expr)` lowers to a new `ictus_ir::Expr::Signed(Box<Expr>,
u32)` -- `expr` itself, unmodified, tagged with its own natural bit width
(computed by the already-existing `expr_width` helper). It is *not* a
general "this value has signed type from here on, tracked through
arbitrary further composition" mechanism the way real Verilog's `signed`
attribute works (where signedness is a property of a net/variable or
expression that propagates through arithmetic and comparison according to
a real type system). Instead, evaluating a `Signed` node directly produces
a specific 64-bit *value*: `expr`'s own bits, with the bit at position
`width - 1` (its sign bit) replicated upward through every bit from
`width` to 63 if set. Every consumer downstream -- a `Select`'s own
masking, or the kernel's commit-time write mask for an assignment target
-- then simply keeps however many of those bits its own narrower context
needs. Because a fixed-width two's-complement bit pattern already *is*
its own correct wider-context representation once sign-extended, this one
mechanical evaluation rule reproduces correct Verilog sign-extension
semantics for every real picorv32 use site (a plain `$signed(x)` RHS, and
`$signed({a,b,...})` with a concatenation argument, both used as -- or,
via `lower_concat_target_assign`, sliced across the parts of -- an
assignment's right-hand side) with no width/context information threaded
through lowering at all: `lower_system_function_call` only ever needs the
*operand's own* width, never the eventual assignment target's.

**Why this is deliberately narrower than "real" signedness, and where
that stops being safe**: `+ & | ^ == != && || !` all happen to be
correct on a `Signed` operand with zero special-casing, because
two's-complement addition, bitwise operators, and equality are bit-for-bit
identical whether the operands are "meant" as signed or unsigned, as long
as they're already extended to a common width -- which evaluating
`Signed` guarantees. Ordering comparisons (`< <= > >=`) are exactly where
this stops being true: `0xFFFF...FFFF` (a sign-extended -1) is a
numerically huge *unsigned* value but must compare as *less than* a small
positive number under signed rules -- genuinely different behavior, not
just a wider bit pattern. Since this kernel doesn't implement a true
signed comparison (reinterpreting the pattern as `i64` and comparing),
`apply_binary_op` explicitly rejects a `Signed` operand of `< <= > >=`
with a clear error rather than silently falling back to the (wrong)
unsigned comparison already implemented -- picked specifically because
picorv32's own ALU does exactly this
(`alu_lts <= $signed(reg_op1) < $signed(reg_op2);`), so the rejection is
confirmed to fire on real source, not a hypothetical worry. Division and
arithmetic right shift (`>>>`) would have the identical problem if/when
they're ever added -- neither is implemented yet at all, so they're
already safely (if incidentally) rejected by `apply_binary_op`'s
catch-all.

**Alternatives considered**: a real signed/unsigned *type* attached to
every `Expr` (or threaded alongside width through `expr_width`) that
every operator consults -- rejected as significant scope for a feature
whose only confirmed real need, so far, is sign-extension into a wider
write target; the value-level marker above gets that exact case right
with a single new `Expr` variant and zero changes to any operator besides
the one guard in `apply_binary_op`, and can still be widened into a real
type system later if a real need for signed comparison/shift/multiply
shows up (which it will -- picorv32's ALU needs exactly that -- see the
roadmap's now-open gap list; this decision is deliberately not the final
design for signedness, just the correct-and-honest v1 slice of it).
Silently treating `$signed(...)` as a no-op (dropping the sign-extension
behavior entirely, always zero-extending) -- rejected immediately as
exactly the silent-wrong-answer failure mode this project's whole
testing discipline exists to prevent; the differential tests specifically
use sign-bit-set values (not just positive ones) to make sure a
regression back to plain zero-extension would be caught.

**Why discovered / confirmed, not assumed**: found the same way as D13
through D15 -- re-running the picorv32 lowering diagnostic after D15
landed surfaced `$signed(...)` as the next blocker, with real usage sites
grepped and read directly (25 occurrences across the file, both the
simple "$signed(plain reference or select)" and
"$signed(concatenation)" forms, and the comparison form that motivated
the `apply_binary_op` guard). Verified with fixtures mirroring both the
plain-reference and concatenation-argument picorv32 styles, checked
differentially against Icarus Verilog using both sign-bit-clear and
several sign-bit-set input values specifically (a zero-extension
regression would only be visible on the latter), plus `ictus_kernel` unit
tests isolating the sign-extension bit manipulation itself, plus a
negative test confirming the signed-comparison rejection fires on a
fixture shaped exactly like picorv32's own `alu_lts` line. Re-running the
diagnostic again after this fix confirms the next real blocker is a
different kind of gap entirely -- task-call statements (`` `assert(...) ``
expands to a call to an empty no-op task in picorv32) -- not another
expression-lowering feature, and is left as an open, deliberately
undecided fork in `docs/roadmap.md` rather than guessed at.

## D17 — Task calls: accept only a provably-empty *callee body*, not any zero-arg call

**Decision**: a task-call statement (`some_task;`) is accepted, and
lowered as a true no-op (zero `ictus_ir::Stmt`s), only when the *called
task's own declared body* is provably empty -- every top-level statement
in it is a no-op, checked recursively through `begin...end` blocks by a
new `statement_is_noop` helper. This resolves the fork D16's closing
paragraph deliberately left open rather than guessed at: the alternative
was to accept *any* zero-argument task call as a no-op, regardless of
what the called task's body actually contains. That more general rule
was rejected. A task call with any arguments at all is rejected
regardless of the callee's body, since v1 has no notion of task ports to
bind arguments to in the first place.

**Why the conservative choice, not the general one**: v1 doesn't model
task execution at all -- no ports, no local variables, no statement
bodies actually run. Accepting "any zero-argument call is a no-op"
would be correct *by observation* for every real call site in picorv32
today (there's exactly one: `` `assert(...) ``'s expansion to
`empty_statement;`), but it would be a rule that happens to work for the
input on hand, not one that's actually verified against the thing it
claims -- a different design (or a later, unnoticed addition to
picorv32's own `empty_statement`, or a different empty-seeming macro
expansion) calling a task that actually sets a signal, waits on a clock
edge, or has any other real behavior would have that behavior silently
discarded, with no error, no warning, nothing -- exactly the
silently-wrong-answer failure mode this project's entire differential-
testing discipline exists to prevent (see
[[ictus-development-practice]]). Checking the callee's actual declared
body costs one extra AST scan (`lower_task_declarations`, structurally
identical to the existing `lower_parameters` scan) and turns that same
hypothetical into a loud, specific rejection (`call to task 'X' is not
supported in v1 -- only a call to a task whose body is provably empty...`)
instead of silence.

**Why the recursive emptiness check, not a shallow one**: the first
implementation attempt checked only `Vec<StatementOrNull>::is_empty()` at
the task's own top level and failed on the *exact* real case this feature
exists for -- picorv32's `empty_statement` task body is written as
`begin end` (one `SeqBlock` statement, itself containing zero statements),
not literally zero statements at the top level. `statement_is_noop`
recurses into `begin...end` blocks (and also accepts a bare null
statement, `;`, with no attributes) specifically to still correctly
reject any *actual* statement no matter how deeply nested inside
otherwise-empty blocks, rather than either failing on this real case (the
shallow check) or accepting too much (e.g. treating a block as empty
without actually checking its contents).

**Why discovered / confirmed, not assumed**: found the same way as D13
through D16 -- re-running the picorv32 diagnostic after D16 landed
surfaced this as the next blocker; the real call site (`` `assert ``'s
macro expansion, picorv32.v line 47, and the task declaration itself,
line 214) was read directly, including noticing the `begin end` body
shape that the shallow-emptiness-check first attempt missed (caught by
actually re-running the diagnostic after that first attempt, not assumed
correct). Verified with a fixture mirroring picorv32's own style
directly, checked both structurally (the call contributes zero
statements) and differentially against Icarus Verilog (surrounding
counter logic is bit-for-bit unaffected by the call), plus two negative
fixtures confirming a task with a real body and a task call with an
argument are both still rejected. Re-running the diagnostic again after
this fix confirms the next blocker is replication/multiple concatenation
(`{N{expr}}`), a data-path expression feature rather than another
statement-form gap.

## D18 — `localparam`/`parameter` share one table; constant grammar reuses `apply_binary_op`

**Decision**: three linked choices, made together while closing
`regindex_bits` (picorv32's `localparam integer regindex_bits =
(ENABLE_REGS_16_31 ? 5 : 4) + ENABLE_IRQ*ENABLE_IRQ_QREGS;`, needing
cross-parameter references, the ternary operator, and multiplication all
at once):

1. **`localparam` and `#(parameter ...)` resolve into one shared name
   table**, via one combined pass (`lower_parameters` now matches both
   `RefNode::ParameterDeclarationParam` and
   `RefNode::LocalParameterDeclarationParam`, which turned out to have
   identical `(Keyword, DataTypeOrImplicit, ListOfParamAssignments)`
   shapes -- different Rust types since sv-parser generates one struct per
   grammar production, but the same fields, factored into one shared
   `resolve_param_assignments` helper). Not two separate tables/passes:
   v1 doesn't support instantiation at all, so the one real difference
   between `parameter` and `localparam` (overridable vs. never) doesn't
   exist as a distinction to preserve yet -- both are simply named
   compile-time constants. This is also *why* cross-parameter references
   work at all: walking the whole module once in source order and
   growing the same table as it goes means a `localparam` (declared in
   the module body) sees every `#(parameter ...)` (declared in the
   header, visited first by source order) already resolved by the time
   its own default is evaluated. A pragmatic "declaration precedes use"
   assumption, not a real dependency solve -- correct for every real case
   found so far, not guaranteed by the Verilog grammar in general (a
   pathological forward-reference would silently resolve wrong rather
   than error, since `PsParameter`'s lookup just wouldn't find the name
   yet and would report "unknown parameter" -- an honest failure, not a
   silent wrong value, so still within this project's normal
   correctness bar even though it's not a complete solve).
2. **The constant-expression grammar walk (`lower_constant_expr`/
   `lower_constant_primary`, new) builds the same `ictus_ir::Expr` the
   general grammar (`lower_expr`) builds, reusing `apply_binary_op`
   directly** rather than writing separate arithmetic for the constant
   context. Verilog's constant-expression grammar
   (`ConstantExpression`/`ConstantPrimary`/...) is a genuinely separate
   parallel AST hierarchy from `Expression`/`Primary` (required wherever
   the language demands a compile-time constant: parameter/localparam
   values, packed-range bounds), so *some* separate walking code is
   unavoidable -- but what each operator symbol *means* (`"+"` ->
   `Expr::Add`, etc.) doesn't need a second definition. This is also why
   multiplication (`Expr::Mul`) and subtraction (`Expr::Sub`) landed as
   real, general operators usable by *both* grammars, not
   constant-expression-only special cases: the general grammar needed
   `-` too (a bit-select target index, `x[regindex_bits-1] <= v;`, is
   lowered through `lower_expr`, not the constant grammar, even though
   `regindex_bits-1` happens to be a compile-time constant), and there
   was no reason to make `*` constant-only once `+`/`-`/`&`/`|`/... were
   already shared.
3. **A new `try_const_fold(&Expr) -> Option<u64>` is the one place that
   decides "is this actually constant, and if so what is it"** --
   deliberately exhaustive over every `Expr` variant (no wildcard arm),
   used both by the constant-grammar walk (where folding is guaranteed to
   succeed, since that grammar can never produce `Expr::Ref`) and,
   separately, by the *general*-grammar call sites that still need a
   compile-time constant: a bit-select target's index
   (`lower_select_target_range`), a part-select bound
   (`lower_constant_index`, unified onto this same helper instead of
   keeping its own separate, less capable one), a packed-range bound
   (`lower_packed_range`), and (for consistency, though not strictly
   required by any real case) the read-side single-bit-select
   (`lower_select`). Before this, several of these call sites each did
   their own narrow "is this literally `Expr::Literal`" check, which is
   *not* the same thing: `regindex_bits-1` lowers (via the ordinary,
   general `lower_expr`/`lower_primary` path used for every expression,
   not a special case) to `Expr::Sub(Expr::Literal, Expr::Literal)` --
   constant, but not itself an `Expr::Literal` -- so the old narrow
   checks would have wrongly rejected it as "variable-indexed," even
   though nothing about it actually depends on a signal.

**Why `try_const_fold` duplicates `ictus_kernel::eval_expr` instead of
calling it**: the two functions do overlapping arithmetic on purpose --
constant folding is a frontend/elaboration-time concern (fold now,
substitute the result, the kernel never sees the original expression),
evaluation is the kernel's runtime concern (re-run every simulated clock
edge) -- different phases of the same project, and `ictus-frontend-verilog`
has no dependency on `ictus-kernel` today (nor should gain one just for
this: the kernel is downstream of the frontend/IR in the dependency
graph, `ictus-cli` is what wires both together, and reaching back
upstream would invert that). Making `eval_expr` `pub` and adding a
frontend -> kernel dependency to avoid ~40 lines of duplicated match arms
was considered and rejected as a worse trade than the duplication itself.

**Why the `ConstantFunctionCall` handling isn't a hack**: found by
actually running picorv32.v through this new code, not assumed --
`ENABLE_REGS_16_31` (a bare parameter reference, no parentheses anywhere
near it) parses as `ConstantPrimary::ConstantFunctionCall` rather than
`::PsParameter`, a genuine `sv-parser` grammar ambiguity for a
zero-argument identifier in constant-expression position (the same
*kind* of finding as D13, a real parser quirk confirmed empirically, not
the same bug). Since v1 never lowers a `function` declaration at all
(confirmed: picorv32.v defines none), a zero-argument "constant function
call" can only ever be a parameter reference the parser classified
differently -- so it's handled exactly like `PsParameter` (a real,
*non*-zero-argument constant function call is still rejected, correctly,
as unsupported).

**Why discovered / confirmed, not assumed**: found the same way as D13
through D17 -- re-running the picorv32 diagnostic after D17 (task calls)
surfaced `regindex_bits` first, then, once that specific line's three
needs were met, the *same* diagnostic loop surfaced `ENABLE_REGS_16_31`'s
parser-ambiguity, then `WITH_PCPI`'s `||`, then `TRACE_BRANCH`'s
concatenation, each fixed and re-verified in turn rather than guessed at
upfront. Verified with a fixture deliberately combining every real shape
found in one place (cross-parameter reference, ternary, multiplication,
a packed-range bound and a bit-select target index both referencing a
localparam) and checked cycle-for-cycle against Icarus Verilog
(`ictus-cli/tests/differential_localparam.rs`) -- Icarus independently
computes the same constant arithmetic from scratch, so an exact trace
match is meaningful evidence of correctness, not just "it didn't error."
Re-running the diagnostic once more after this fix confirms the next
blocker is unrelated to constants at all: a 4-state `x`/`z` digit in a
literal outside a `case` item, which needs a real policy decision (what
does `x` even mean in a 2-state kernel, D6) before implementation.

## D19 — A 4-state `x`/`z` literal digit (outside `case`) resolves to `0`

**Decision**: a `x`/`z` digit in a numeric literal -- whole-value
(`8'bx`, `8'hxx`, `8'dx`) or mixed with real digits (`4'b10x1`), in any
base -- resolves to the bit `0` when it's lowered *outside* a
`case`/`casez`/`casex` item's own wildcard matching (which already has
separate, correct handling -- `lower_wildcard_binary`, tracking a
`care_mask` for pattern matching, not a value). This is a narrow,
self-contained literal-*parsing* policy, not the real per-signal 4-state
*tracking* D6 describes as a genuine future capability (opt-in, per
signal/module, tracking propagate-through-simulation unknown/high-Z
state) -- nothing about D6's scope or sequencing changes here; this
decision only says what a specific source character parses to as a
constant, today, with the 2-state representation D6 already committed to
for v1.

**Why 0, specifically**: matches Verilator's own default X-handling
policy -- a real precedent for exactly this choice in exactly this kind
of tool (a 2-state-by-default simulator that still needs to accept real
RTL containing `x`/`z` literals), not a guess invented for this project.
picorv32 itself uses `x` literals in two distinct, common, legitimate
styles that motivate accepting them at all rather than just rejecting
them louder: an all-`x` "don't care" output on a dead/inactive code path
(`assign pcpi_mul_rd = 32'bx;`, a PCPI co-processor output when that
path isn't the active configuration) and a "default to `x`, then
override in every `case` arm" idiom (`decoded_imm <= 1'bx;` immediately
followed by a `case` covering every real instruction encoding) --
extremely common, idiomatic synthesizable-Verilog style, not unusual
source this project should expect to rewrite around.

**Alternatives considered**: keep rejecting `x`/`z` outright (extending
the existing `DecimalNumber::BaseXNumber`/`BaseZNumber` rejection to
binary/hex too, just with a clearer error) -- rejected because it
doesn't actually unblock anything; the whole point of running real
designs (picorv32) is lowering the source as written, and this specific
idiom is too common in real synthesizable RTL to treat as "the source
needs to be rewritten." Some other resolved value (e.g. `1`, or
alternating/random bits) -- rejected as having no comparable real-world
precedent and no clearer justification than `0`; `0` at least matches
what the closest prior-art tool (Verilator) actually does.

**Why this can't be verified with a general differential test, and what
that implies for how it's tested**: a genuinely-`x` result in a *real*
4-state reference simulator (Icarus) has no single value to compare
against at all -- it reports `x`, which doesn't parse as the plain
integers this project's differential tests already compare, and even if
it did, "Ictus says 0, Icarus says x" is not a disagreement about
correctness, just two different (both individually consistent) answers
to a question the Verilog LRM deliberately leaves implementation-defined
for a 2-state tool. So the resolution policy itself is verified
structurally (`ictus-frontend-verilog/tests/xz_literal.rs`, asserting
the exact resolved `Expr::Literal` value), and a *separate* differential
test is built specifically around the one shape where an `x`-containing
design is still safe to compare end-to-end: the
default-then-always-overridden idiom, where the final observed value is
identical in both simulators regardless of how (or whether) the `x`
default was ever resolved (`ictus-cli/tests/differential_xz_default.rs`,
using a `case` that covers every possible selector value so the `x`
default is provably never the sampled result in either simulator).

**Why discovered / confirmed, not assumed**: found the same way as D13
through D18 -- re-running the picorv32 diagnostic after D18 (localparam)
landed surfaced this next; real usage sites (both the dead-code and
default-then-override styles) were grepped and read directly rather than
guessed at, and the "resolves to 0" policy was cross-checked against
Verilator's documented default behavior before committing to it, not
invented from scratch. Re-running the diagnostic again after this fix
(and, immediately after it, a related but separate fix letting
comparison/logical/reduction results -- always exactly 1 bit -- be used
as concatenation operands, found the moment picorv32's own
instruction-decode concatenations could be reached) confirms the next
blocker is shift operators (`<< >>`, and the arithmetic variants
`<<< >>>`), not implemented at all yet.

## D20 — `>>>` needs a `Signed` operand; `>>` of one is rejected

**Decision**: the shift operators split three ways along the same
signed/unsigned axis D16's guard already reasons about, rather than
getting one uniform treatment:

1. `<<` and `<<<` both lower to `Expr::Shl`, with a `Signed` operand
   passing straight through. Shifting *left* has no sign behavior to
   differ about -- Verilog's "arithmetic" left shift is bit-for-bit the
   logical one -- so these join `+`/`-`/`*` in the category of operators
   a sign-extended operand is simply safe for.
2. `>>` lowers to `Expr::Shr`, but **rejects** a `Signed` left operand.
   Verilog's `>>` zero-fills even for a signed value (that difference is
   the entire reason `>>>` exists), and an `Expr::Signed` value arrives
   already sign-extended across all 64 bits -- so a logical shift of it
   would pull those extension bits down into the result in place of the
   zeros the language specifies. Rejected rather than silently doing
   that, exactly as an ordering comparison of a `Signed` operand is.
3. `>>>` lowers to `Expr::AShr` **only** when its left operand is
   `Signed`, and to an ordinary `Expr::Shr` otherwise. This is the one
   operator where a `Signed` operand is actively *required* rather than
   merely tolerated -- and the fallback isn't a guess or a degradation:
   Verilog itself defines `>>>` on an *unsigned* operand as an ordinary
   logical shift, so lowering it that way is the LRM's own rule.

**Why `AShr` can be a plain `i64` shift**: only because of (3)'s
restriction. An `Expr::Signed` operand has already been sign-extended
across the full 64 bits (D16), so the bit an `i64` arithmetic shift
replicates *is* the operand's real sign bit rather than whatever happened
to land in bit 63. That's also why this doesn't need `AShr` to carry a
width the way `Signed`/`BitwiseNot`/the reduction operators do -- the
width information is already baked into the operand by the time it gets
here. Sign-extending further than the source width and then truncating at
write time gives the same low bits as sign-extending exactly to the
context width and shifting there, which is what makes
`$signed(x) >>> n` assigned into a *wider* target come out correct too
(verified against Icarus, not assumed -- see
`ictus-cli/tests/differential_shift.rs`'s 16-bit output column).

**Alternatives considered**: evaluate `AShr` as an `i64` shift
*unconditionally*, without requiring a `Signed` operand -- rejected as
accidental correctness: it would happen to work for every realistic
design only because signals narrower than 64 bits always leave bit 63
clear, which is exactly the "works because of what we happen to feed it"
reasoning this project treats as a latent bug rather than a design.
Rejecting unsigned `>>>` outright instead of lowering it to `Shr` --
rejected because it's well-defined legal Verilog, and refusing it would
be inventing a restriction the language doesn't have. Giving `<<<` its
own IR variant -- rejected as pure duplication, since it's defined to be
identical to `<<`.

**Out-of-range shift amounts**: a shift of 64 or more produces 0 for the
logical shifts and a full sign-replication for the arithmetic one. Worth
recording because the two obvious Rust spellings are both *wrong* here: a
bare `<<`/`>>` on a `u64` panics on shift-overflow, and `wrapping_shl`
silently takes the shift amount modulo 64 -- turning `x << 64` into
`x << 0`, i.e. `x` unchanged, when Verilog says every bit is shifted out.
`checked_shl`/`checked_shr` with an explicit `unwrap_or(0)`, and a clamp
to 63 for the arithmetic shift, are what actually match the language.
Covered by `ictus_kernel`'s own unit tests rather than differentially: a
realistic design's shift-amount signal is only a few bits wide, so no
differential fixture can reach the case at all.

**Why discovered / confirmed, not assumed**: found the same way as D13
through D19 -- re-running the picorv32 diagnostic after D19 surfaced
`unsupported binary operator '<<'`, and every real usage site was grepped
and read before designing (`reg_op1 << reg_op2[4:0]`, `reg_op1 >> 4`,
`$signed(reg_op1) >>> 4`, `$signed({...}) >>> reg_op2[4:0]` -- notably,
picorv32 writes `>>>` *only* ever with an explicit `$signed(...)` left
operand, which is what made (3)'s split the obvious shape rather than a
speculative one). The differential test deliberately picks values with
the sign bit set for three of its four cases, so an arithmetic and a
logical right shift give visibly different answers and a regression
would fail rather than slip through. Re-running the diagnostic once more
after this fix lands on D16's own signed-ordering-comparison guard
(picorv32's `alu_lts <= $signed(reg_op1) < $signed(reg_op2);`) -- a
deliberately-deferred gap finally reached, not a new discovery.

## D21 — Signed comparison: one `SignedLt`, and the mixed case stays rejected

**Decision**: signed ordering comparisons are supported when **both**
operands are `Signed`, via a single new `Expr::SignedLt` variant; the
other three orderings are composed from it at lowering time (`a > b` is
`b < a`; `a <= b` is `!(b < a)`; `a >= b` is `!(a < b)`). A comparison
with exactly **one** `Signed` operand stays rejected. This closes the gap
D16 opened deliberately, and does it along the same signed/unsigned axis
D20 split the shift operators on.

**Why one variant, not four**: the three identities hold exactly for
integers (no NaN-like case to worry about), so four near-identical
variants would be pure duplication -- the same reasoning that made
`~&x` compose from `BitwiseNot` + `ReduceAnd` rather than getting its own
node, and unsigned `>>>` reuse `Shr`. Evaluating `SignedLt` as a plain
`i64` comparison is correct for exactly the reason `AShr`'s `i64` shift
is: the both-operands-`Signed` restriction guarantees each side arrives
sign-extended across all 64 bits, so the sign an `i64` comparison reads
is the operand's real one, not whatever landed in bit 63.

**Why the mixed case stays rejected -- the real fork here**: Verilog does
define it (a comparison with any unsigned operand is performed
*unsigned*), so this isn't a gap in the language's own answer. It's a gap
in ours: performing it correctly needs the signed operand truncated back
to its own declared width first, since an 8-bit `-1` has to read as
`255`, not as the 64-bit sign-extended pattern `Expr::Signed` evaluates
to. `Expr::Signed` does carry the width that truncation would need, so
this is implementable -- it just isn't implemented, and no real design
has needed it (picorv32's only signed comparison has both operands
signed). Rejected with an error that names the reason rather than
silently comparing that sign-extended pattern as an enormous positive
number, which is the outcome D16 was guarding against in the first place.

**Why discovered / confirmed, not assumed**: the scope came from grepping
every `$signed` usage in picorv32 that touches a comparison -- there is
exactly one shape, `$signed(a) < $signed(b)`, both operands signed, which
is what made the both-signed/mixed split the obvious design rather than a
speculative one. The differential test drives values where exactly one
operand's sign bit is set, so the signed and unsigned readings genuinely
disagree and an unsigned-control column comes out *opposite* to the
signed result -- a regression that quietly compared unsigned would fail
rather than pass. Re-running the diagnostic after this fix reaches
`cpuregs`, picorv32's register file: array/memory signals with a runtime
index on both read and write sides, the first blocker since the
`localparam` work that genuinely can't be handled by widening expression
lowering.

## D22 — Arrays: a `depth` on `Signal`, a separate write statement, a side table

**Decision**: array/memory signals (`reg [31:0] mem [0:31];`) are modelled
by three choices made together:

1. **`Signal` gains `depth: Option<u32>`** rather than arrays getting a
   separate declaration kind or id space. `None` is everything that
   existed before, unchanged; `Some(n)` is `n` elements of `width` bits.
   Keeping `width` as the *element* width means every existing piece of
   code that reasoned about a signal's width -- masking on write,
   `expr_width`, the concatenation machinery -- kept working untouched.
2. **A separate `Stmt::ArrayAssign`**, not another field on
   `NonBlockingAssign`. The two really are different writes: one
   addresses an element chosen at simulation time and replaces it whole,
   the other a fixed signal and possibly just a constant bit range of it.
   A combined statement would have made every scalar assignment carry a
   `None` index, and would have made the nonsensical combination
   (element index *and* bit range) representable and in need of a runtime
   check. As a separate statement, "no bit range of an array element" is
   structural. It also left every existing `NonBlockingAssign`
   pattern-match in the test suite untouched.
3. **Array contents live in a side table in the kernel**
   (`arrays: Vec<Vec<u64>>`, indexed by the same `SignalId` as the scalar
   values, an empty `Vec` for every scalar) rather than being packed into
   the flat value buffer with per-signal offsets. `eval_expr` gained an
   `arrays` parameter to reach it.

**Why the side table, given architecture.md wants contiguous storage**:
packing arrays into the one flat buffer would mean every signal access
going through an offset table -- changing `values[id]` (the common case,
every scalar read in the interpreter) into an indirection, to benefit a
layout that *this* kernel doesn't need. D12 already settled that this
interpreter exists to be a correctness baseline, not the fast path; the
bit-packed contiguous layout is the compiled kernel's concern, and it
will be replacing these internals wholesale rather than inheriting them.
Choosing the layout that keeps the baseline simple and obviously correct
is the right trade here; choosing the "eventually fast" one would be
optimizing the thing that's explicitly slated for replacement.

**The index is evaluated during the evaluation phase, not at commit**:
`mem[addr] <= x;` has to use `addr`'s value from *before* the edge, the
same as the right-hand side does -- so the pending write records an
already-evaluated `u64` index, not the expression. Re-evaluating it at
commit time would read `addr` after earlier writes in the same tick had
landed, which is exactly the non-blocking-semantics bug D14's commit-loop
reasoning was careful to avoid on the scalar side.

**Out-of-range: read 0, drop the write.** The read half follows
`DynamicBitSelect`'s existing precedent for an out-of-range bit index
(D6-adjacent: a 2-state kernel has no 'x' to return). The write half is
the one with a real alternative -- clamping or wrapping the index -- and
both were rejected because they'd corrupt a *different*, innocent
element. Doing nothing at least confines the damage to the write that was
already out of range.

**Deliberately rejected, each with a test**: an array as a module port
(nothing in v1 can address it from outside), a bit-select of an array
element (unrepresentable on the write side per (2), and allowing it only
on reads would be a confusing asymmetry), an array declared alongside
other names in one statement (the unpacked dimension is found by
searching the declaration and can't be attributed to one name --
`reg [7:0] a, mem [0:3];` would otherwise make `a` an array too), a
continuous `assign` to an element (`ictus_ir::Assign` names one whole
signal, and `settle_combinational` has no commit phase to resolve an
index in), and any unpacked form other than `[high:low]`.

**Why discovered / confirmed, not assumed**: the target shape came from
reading picorv32's actual register file and every use of it before
designing anything -- declaration, the runtime-indexed writes
(`cpuregs[latched_rd] <= ...`, including one with an *expression* index,
`cpuregs[latched_rd ^ 1]`), and both constant- and runtime-indexed reads.
The differential test rewrites an element while reading it, so the
pre-edge read timing is actually exercised rather than assumed, and reads
a neighbour afterwards so writing the wrong slot would surface. It fills
the array in an unsampled warm-up phase first, because a never-written
element reads 'x' in Icarus and 0 here -- the same 2-state/4-state
boundary D19 ran into, handled the same way. Re-running the diagnostic
after this lands reaches blocking assignment (`=`), which is not another
statement form but a second *write discipline* that has to coexist with
this kernel's evaluate-then-commit model -- flagged in roadmap.md as
needing its own decision.

## D23 — Blocking assignment: a second write discipline, applied through one shared write path

**Decision**: blocking assignment (`=` inside a clocked block) is
implemented by letting the statement-evaluation phase mutate live state
directly, alongside the existing queue of deferred writes — and the
two kinds get separate IR statements (`Stmt::BlockingAssign`,
`Stmt::BlockingArrayAssign`) rather than a `blocking: bool` flag on the
existing ones.

**Background, in Verilog's terms**: a clocked block contains two kinds of
assignment that look almost identical and behave differently.
`x <= value;` (non-blocking) schedules the write; every read in that same
block still sees the *old* `x`, which is what lets a shift register be
written as a plain sequence of statements without each stage clobbering
the next. `x = value;` (blocking) writes immediately, so the next
statement reads the new value — it is used as a local variable in the
middle of a sequence. The LRM models this as two regions of one time
step: active (where blocking writes happen, in statement order) and NBA
(where non-blocking writes land, afterwards).

**Why this needed a decision at all**: every write this kernel had
modelled until now was deferred. `tick()` evaluated all processes against
a frozen snapshot, collected pending writes, then committed them. A
blocking write cannot go through that path — deferring it would make the
very next statement read a stale value, silently computing the wrong
answer rather than failing. So the evaluation phase had to become capable
of mutating state, which it previously was not.

**How the two coexist**: `eval_stmts` now takes `&mut` access to the
value buffer and the array side table *and* the pending-write list.
A blocking statement calls `apply_write` immediately; a non-blocking one
pushes onto the list as before, and the list is drained through the same
`apply_write` after every process has run. That falls out of the LRM's
own ordering rather than approximating it, and it gets both of the cases
that distinguish the two kinds right without either being special-cased:

* `a = 5; b <= a;` — `b` is scheduled with 5, because the blocking write
  already landed in the live buffer that the right-hand side reads.
* `x <= 1; y = x;` — `y` gets the *old* `x`, because the non-blocking
  write is still sitting in the pending list, untouched by anything the
  active region reads.

**Why one `apply_write` rather than two write paths**: the read-modify-
write for a partial (bit-range) target, the width masking, and the
out-of-range array rule are subtle enough that two copies would drift.
Extracting them means the only thing that differs between the two
disciplines is *when* the function is called — which is exactly the
distinction the LRM draws, and nothing else.

**Why separate statements rather than a flag** (the narrow reading of the
principle in D15/D20 — separate variants for different *addressing*,
composition or flags for different *timing* — would have said flag):
three reasons outweighed it. `Stmt::Assign` as a name collides with the
existing top-level continuous-`assign` struct, so the variants would have
had to be named awkwardly. A match arm reading `BlockingAssign` states
the timing at the place a reader needs it, where `NonBlockingAssign { .. }`
whose meaning inverts on a field several lines down does not. And the
cost the principle exists to avoid — duplicated *behaviour* — is already
avoided by the shared `apply_write`; what separate variants actually cost
here is a duplicated field list.

**Deliberately rejected, with a test**: compound assignment (`acc += in;`).
sv-parser routes it through the same `OperatorAssignment` node as `=`,
differing only in the operator symbol, so accepting the node blindly would
have treated `+=` as `=` and silently dropped the accumulate. The operator
is checked explicitly and anything but `=` is rejected by name.

**Why discovered / confirmed, not assumed**: the gap came from the
picorv32 diagnostic — `set_mem_do_rinst = 1;` sitting a few lines from
`decoder_trigger <= 0;` inside one `always @(posedge clk)` block, which is
also why the fixture mixes both kinds in one block rather than testing
them apart. The differential test is the load-bearing one: structural
tests can only show the two kinds stayed distinguishable through
lowering, whereas the difference *is* the timing. It compares a blocking
write read by the next statement, a non-blocking right-hand side reading a
just-blocking-written signal, and — the case that fails if the two
disciplines are collapsed — a blocking read of a signal a non-blocking
write targeted two statements earlier, which must still see the pre-edge
value. Its first cycle is unsampled for the usual 2-state/4-state reason
(D19). Re-running the diagnostic after this lands reaches a narrower gap:
`expr_width` can't determine the width of a bitwise `And` used as a
concatenation operand.

## D24 — Self-determined width for binary bitwise and arithmetic results

**Decision**: `expr_width` now answers for `& | ^ + - *`, returning the
wider of the two operands' widths — Verilog's own self-determined width
rule. Shifts stay rejected.

**First, a correction to how this gap was recorded.** D23 and the roadmap
described it as a *concatenation* operand problem, because the error text
said "concatenation operand's width can't be determined". It wasn't.
`expr_width` has two callers, and the one picorv32 actually tripped is
the other: `|(irq_pending & ~irq_mask)` at picorv32.v:1538 — a reduction
operator, which needs its operand's width to know how many bits to fold.
The error message named only the caller it was originally written for,
which made the diagnosis wrong in a way that would have pointed the fix
at the wrong place. The message now describes what the function is for
rather than one of its callers, which is the actual lesson: an error
raised by a shared helper should not name one caller's context as if it
were the only one.

**The rule**: for `a OP b`, Verilog extends both operands to
`max(width(a), width(b))`, performs the operation there, and truncates
the result back to that width. This is not an approximation — it's what
the LRM specifies for a self-determined context, which is what both of
`expr_width`'s callers are.

**Why the bitwise and arithmetic operators went in together** (they were
initially considered separately, and the difference between them is worth
recording rather than smoothing over): for `& | ^` the rule is exact in
the strongest sense — neither operand can contribute a set bit above its
own width, so nothing can be lost and the answer is right regardless of
what any consumer does with it. For `+ - *` the width is the same rule
but the result genuinely can exceed it, and Verilog *discards the
overflow*. `{a + b, c}` with 4-bit `a` and `b` packs four bits, so a
carry out of the adder is simply gone; getting it requires widening an
operand first (`{1'b0, a} + {1'b0, b}`). That is a real trap, and it was
the argument for leaving arithmetic rejected — a loud error is better
than a silently dropped carry.

It went in anyway, deliberately: rejecting `a + b` here would not have
prevented the trap, only moved it. The same truncation already happens
whenever `a + b` is assigned to a register narrower than the sum, which
this frontend has always supported. Refusing it in *these* two positions
while allowing it everywhere else would be an inconsistency the user has
to discover, not a safeguard. Matching Verilog uniformly, and naming the
trap in the code where the decision is made, is the honest version.

**What makes the arithmetic case actually correct**, rather than correct
by assumption: the kernel evaluates on `u64`, so an `Add` result really
does carry bits above the reported width. Every consumer of `expr_width`
masks the evaluated value back down to it — concatenation packing,
`BitwiseNot`, and all three reduction operators (see
`ictus_kernel::eval_expr`). The truncation happens; it isn't inferred.

**Why shifts stay rejected**: Verilog gives `a << b` its **left**
operand's width, not the max of the two — a genuinely different rule, not
the same change applied twice. Folding them in on the assumption the
rules match is exactly the guess this project declines to make, so they
are rejected with an error that names the difference, and there is a test
for it.

**`constant_expr_width` got the same arms**, so a `localparam` whose
value concatenates `A + B` doesn't fail where the identical expression in
a general context succeeds.

**Why discovered / confirmed, not assumed**: the usage site was located
in picorv32 before choosing anything (`irq_mask`/`irq_pending`, declared
consecutively at lines 198-199, confirming the signal ids in the error),
which is what revealed the misdiagnosis above. The differential test
against Icarus pins the truncation cases specifically — `a = 12, b = 10`
gives `{a + b, 2'b11}` = 27, not the 91 an untruncated 5-bit sum would
give.

**With this, the whole of picorv32.v lowers cleanly** — 225 signals, no
error — which is the first time the real design has made it through the
frontend end to end. That closes the diagnostic loop that has driven
every increment since D13. It says nothing yet about whether the design
*simulates* correctly, which is a different and much larger question, and
the roadmap now frames that as the next thing to establish.

## D25 — Running the real design: trace replay, and three defects it found

**Decision**: correctness at design scale is established by *replaying a
recorded trace*, not by co-simulating two hand-written testbenches. Icarus
runs a testbench that wraps picorv32 in a memory and records both what it
drove into the core and what the core produced; the Rust side replays the
recorded inputs into Ictus and compares the outputs.

**Why that shape**: Ictus can't instantiate modules, so it can't run the
testbench. The obvious alternative -- reimplement the memory model in Rust
-- would put two hand-written models on the two sides of the comparison,
and every disagreement between *them* would look exactly like a simulator
bug. Replaying a recording makes the stimulus identical by construction,
so a difference can only mean the two simulators read the same design
differently. It also costs nothing in fidelity: the recording is of a real
memory model, just not one that had to be written twice.

**Sampling and timing**: the testbench samples at each negedge, where
nothing is moving, so one recorded row holds the core's outputs after edge
N *and* the inputs it will see at edge N+1. Replaying a row therefore
takes two steps -- drive the previous row's inputs and tick (registers
sample what was on the wire before the edge), then drive this row's inputs
before reading (a combinational output reacts to an input with no edge in
between). Getting this backwards moves every combinational output by a
full cycle, which is not a subtle inaccuracy.

**What it found.** Three defects, and the common thread is what matters:
every one of them let the design lower cleanly, run without error, and
produce plausible output while being wrong. None was reachable by reading
the code or by any fixture-sized test.

1. **`Simulation::set` did not re-settle combinational logic.** A driven
   input feeds continuous assignments with no clock edge in between --
   picorv32's `assign mem_xfer = mem_valid && mem_ready;` is exactly this
   -- so a `get` between ticks returned a value computed from the
   *previous* inputs. `set` now settles, rather than exposing a separate
   `settle()` the caller must remember: forgetting it would be silent, and
   the cost is one pass over the module's `assign`s on a kernel that
   D12 already designates the correctness baseline rather than the fast
   path.

2. **Net declarations with an initializer were silently dropped.**
   `wire mem_done = ...;` is defined by IEEE 1800 as exactly
   `wire mem_done; assign mem_done = ...;`, and picorv32 uses that
   spelling for most of its combinational logic -- 59 declarations against
   43 standalone `assign` statements. The frontend created the wires and
   never drove them, so they read 0 forever. A *variable* initializer
   (`reg x = 0;`) looks identical and means something entirely different
   -- it runs once before time zero rather than driving the signal
   continuously -- so it is now rejected outright instead of being
   mis-lowered or quietly ignored, since ignoring is right only for `= 0`.

3. **Binary operator precedence was never applied.** `sv-parser` returns
   a binary expression as a right-leaning chain in source order:
   `a == b && c == d` arrives as `a == (b && (c == d))`. Lowering that
   literally computes a different value than Verilog specifies. picorv32's
   instruction decoder is built entirely from this shape
   (`rdata[14:12] == 3'b001 && rdata[31:25] == 7'b0000000`), so the core
   decoded `addi` as a shift instruction -- and kept running. The narrow
   ternary fix from D13 is now subsumed: `?:` binds looser than every
   binary operator, so a bare ternary can only end a chain and everything
   left of it is really its condition, which falls out of the general
   precedence rule rather than needing its own hand-written case. The same
   rule also fixed an unnoticed associativity bug: `a - b - c` was being
   lowered as `a - (b - c)`.

**The most useful thing it found isn't a defect.** After all three fixes
the port comparison passes exactly -- every traced output, every cycle --
and picorv32's register file in Ictus is still empty. The core reproduces
the bus trace while executing nothing, because `cpuregs_write` and
`cpuregs_wrdata` are driven from an `always @*` block the frontend does
not lower. For straight-line code the fetch addresses don't depend on any
register value, so a dead datapath is invisible from the ports.

That is worth stating plainly: **agreement on a design's ports is not
evidence that the design ran.** The differential test says so in its own
comments and asserts the register file is *still* empty, so the assertion
fails loudly the moment `always @*` lands and the real comparison can
replace it.

**Unknown values**: a field Icarus reports as `x` is skipped rather than
compared, the same 2-state/4-state boundary as D19. The test asserts a
floor on how many points were actually compared, so it can't erode into
something that skips everything and passes.

**Known limitation this exposed, not fixed**: `settle_combinational`
evaluates each continuous assignment once, in source order. Source order
is not guaranteed to be a correct evaluation order -- an assignment
reading a net driven further down would see a stale value for a cycle.
Both spellings of continuous assignment are now collected in one pass so
that source order is at least *preserved* (collecting them separately
would have reordered every design's logic for no reason), but settling to
a real fixpoint is the actual fix and is on the roadmap.

**Next**: `always @*`. It is currently ignored rather than rejected, which
is the same silent-drop failure as (2) above and is called out as such --
rejecting it today would stop picorv32 lowering at all and take the
differential test with it, so it is the immediate next increment rather
than a deferred one.

## D26 — Combinational `always` blocks, and settling to a fixpoint

**Decision**: `always @*`, `always @(*)` and `always_comb` lower to a new
`ictus_ir::CombProcess`, and the kernel settles all combinational logic --
continuous assignments and these blocks together -- by iterating to a
fixpoint instead of making one pass in declaration order.

**With this, picorv32 executes.** The bus trace already matched (D25);
now the register file matches Icarus too, across a program that does ALU
work, a store, a load and a branch. That was the whole point of the
increment: picorv32 computes its register writes (`cpuregs_write`,
`cpuregs_wrdata`) inside an `always @*`, so with those blocks ignored the
core reproduced every bus cycle while writing no registers at all.

**No sensitivity list is stored.** All three spellings mean "re-run
whenever anything this block reads changes", so the list is implied by
the body; recording one would create a second source of truth that could
disagree with it. An **explicit** list (`always @(a or b)`) is rejected
rather than quietly widened to `@*`. That is not pedantry: an incomplete
sensitivity list is a classic Verilog bug, a conforming simulator honours
the list exactly as written, and silently widening it would make this
simulator disagree with the reference on precisely the designs where the
difference matters.

**The body needed no new machinery.** These blocks are `if`/`case` plus
*blocking* assignment, all of which landed in earlier increments -- the
blocking-assignment work (D23) turns out to have been the prerequisite,
since "a later statement sees what an earlier one wrote" is the whole
execution model of a combinational block. A *non-blocking* assignment
inside one is rejected: it is legal Verilog with specific meaning (the
write defers past the block's own later statements), and combinational
settling happens outside any clock edge with no deferral phase to put it
in. Treating it as blocking would be the silent-wrong-answer case.

**Why a fixpoint, not an ordering.** A single pass is correct only when
the source happens to be written in dependency order. Continuous
assignments made that *usually* true; `always @*` blocks make it much
easier to violate, since two blocks can feed each other in either
direction and no source order is right for every design. Iterating until
a whole pass changes nothing gives the same answer regardless of order,
and removes a class of bug rather than documenting it.

**The subtle part, which this got wrong first**: convergence must be
decided by comparing the state before and after a *whole pass*, not by
asking each write whether it changed anything. Combinational Verilog's
dominant idiom is to assign a default and then override it:

```verilog
always @* begin
    cpuregs_write = 0;
    if (...) cpuregs_write = 1;
end
```

Every pass over that writes the signal twice and can end exactly where it
began. A per-write "did this change something" flag is therefore true
forever, and settling never terminates -- which is how the first version
reported picorv32 as having a combinational loop. The per-write flag was
the obvious optimization and it was wrong; the state comparison costs a
clone per pass, which this interpreter can afford (D12).

**Non-convergence panics**, naming the signals still moving. A design
whose combinational logic has no stable answer has a real bug, and a
conventional simulator surfaces it as a hang or an oscillating waveform;
saying which signals are oscillating is more useful than either.

**Inferred latches fall out, and are not special-cased.** A block that
doesn't assign its target on every path leaves the previous value in
place, which is exactly Verilog's inferred latch -- nothing had to
implement it, and nothing flags it. Flagging was considered (many linters
do) and rejected: this is a simulator, and a latch is legal, simulable
Verilog whose meaning is unambiguous.

**A latch did force one API addition**, `Simulation::set_all`. For
ordinary combinational logic, driving inputs one at a time and settling
between them is indistinguishable from driving them together -- settling
is a function of the final inputs. For a latch it is not: lowering the
enable *after* changing the data lets the latch capture the new data
first. Verilog draws the same distinction by whether simulation time
advanced between the two assignments, and a testbench writing a group of
inputs in one instant gets the "together" behaviour. `set_all` reproduces
that; `set` remains a one-signal convenience built on it. This surfaced
as a genuine Icarus/Ictus disagreement in the new differential test, not
as a theory.

**Also rejected rather than skipped, now**: `negedge` blocks,
`always_ff`, `always_latch`. Every `always` construct now lowers to
something or errors. Previously anything that wasn't `posedge` was
silently ignored, which is the failure mode D25 was written about -- it
is what hid `always @*` in the first place.

**String literals** came along because reaching picorv32's `always @*`
blocks finally reached its disassembly block (`new_ascii_instr = "lui";`,
a signal that exists for waveform viewing). A string literal in an
expression is not a string type: IEEE 1800 §5.9 defines it as its
characters packed 8 bits each, so `"lui"` is a 24-bit `0x6C7569`. Capped
at 8 characters, since longer genuinely does not fit this kernel's `u64`
values -- rejected with that reason rather than truncated, a silently
truncated string being both wrong and hard to notice. picorv32's longest
is exactly 8, into a `reg [63:0]`.

## D27 — Elaborating `generate if`, and one driver per signal

**Decision**: `generate if` conditions are evaluated at lowering time
against the resolved parameters, and only the selected branch is lowered.
Module instantiation, `generate for` and `generate case` are rejected
where they occur in code that exists, rather than skipped. And a signal
may now be driven from only one place.

**The bug, and how it was found.** The frontend walks a module with
sv-parser's deep iterator, which descends into a `generate if` and yields
the contents of *both* branches. So the frontend lowered both. picorv32's
`generate if (TWO_CYCLE_ALU)` became a clocked ALU and a combinational
ALU driving the same signals, and the design was correct only because
settling runs the combinational one last. Its `generate if (ENABLE_MUL)`
branches hold module instantiations, which were silently dropped, leaving
only the `else` branch's tie-off assignments -- which happen to be right
for the default parameters.

No test found this, and none could have with default parameters on
straight-line code. It was found by starting on the obvious next step --
running picorv32's own test firmware -- and noticing that the firmware
was built for non-default parameters, which raised the question of how
parameters select code at all. The answer was "they don't". This is the
fourth instance of the pattern in D25/D26: a design that lowers, runs,
and produces right answers *by coincidence*.

**Why a filter over every walk, not a transformed tree.** Every walk in
the frontend iterates the module independently (parameters, tasks,
declarations, always blocks, assignments). Elaboration records the source
spans of the unselected branches, and each walk skips any item whose
first source position lies inside one. Producing a pruned copy of
sv-parser's tree would have been cleaner in principle and much larger in
practice; the filter is one check per walk, and the spans use the same
byte offsets every node in the tree already carries.

**Order matters inside elaboration itself.** Constructs are visited
outer-before-inner, and one inside an already-excluded branch is skipped
rather than evaluated. `else if` needs nothing special as a result -- the
nested `if` sits inside the outer `else` and is either excluded with it or
evaluated in its own right. Skipping also means an unselected branch may
contain anything at all, including things this frontend would reject;
rejecting code that doesn't exist in the elaborated design would be as
wrong as lowering it.

**What is rejected, and why each isn't approximated:**

- **Module instantiation.** Previously dropped silently, which is the
  worst available behaviour: picorv32 with `ENABLE_MUL=1` would have
  lowered without error and had no multiplier. Instantiation is a real,
  planned feature, not something to approximate by omission.
- **`generate for` / `generate case`.** Legitimate, not needed yet; a
  `for` also needs genvar scoping.
- **A parameter declared inside any `generate if`**, selected or not.
  Parameters are resolved before branches are chosen, so the common idiom
  of declaring the same name in both branches would silently take
  whichever came last. The fixture for this is exactly that idiom, and
  without the check it would lower to the wrong shift amount.

**One driver per signal.** After lowering, a signal driven by more than
one clocked process, combinational process or continuous assignment in
total is rejected. This is the invariant the generate bug broke, and
checking it would have refused the old lowering outright instead of
depending on a test that happened to sample between clock edges. The
correctly elaborated picorv32 has no multiply-driven signal. It is
deliberately stricter than Verilog: two continuous drivers on a net are
legal and resolved by net type (a conflict reads `x`), which a 2-state
kernel cannot represent; two `always` blocks writing one variable are
legal but race, and synthesis refuses them. The legal *and* well-defined
case it turns away -- two blocks writing disjoint bit ranges or different
array elements of one signal -- would need per-bit driver tracking to
accept safely.

**Verified by breaking it.** The differential test's fixture has two
`generate if`s selecting *opposite* branches, so a registered `sum` and a
combinational `diff` coexist, and it samples between edges as well as
after them. With elaboration temporarily disabled, only the between-edge
samples of `sum` disagree with Icarus -- every post-edge sample still
matches, because right after an edge a registered and a combinational
`a + b` agree. A post-edge-only test would have passed the bug. The
`else if` chain in the same fixture came out *right* even with every
branch lowered, because the last of three competing assignments wins each
settling pass -- the same luck, one more time.

**Next.** A RISC-V cross-compiler is available (`riscv64-unknown-elf-gcc`,
which targets rv32 with the right `-march`/`-mabi`), so picorv32's own
per-instruction tests can be assembled for its *default* configuration --
base ISA, no multiply, divide, interrupts or compressed instructions --
and run through the existing trace harness. That is the longer, more
demanding program the roadmap called for, and it no longer waits on
parameter overrides or instantiation.

## D28 — picorv32's own instruction tests, and a clean result

**Decision**: run the riscv-tests rv32ui suite that ships with picorv32 --
37 programs, one per base RV32I instruction, about 49,000 cycles in all --
through the same trace-replay harness as D25, and commit the assembled
images so the test suite needs no RISC-V toolchain.

**The result is that nothing failed.** All 37 match Icarus on every
traced port, every cycle, and on the final register file. That is worth
recording plainly, because it is the first time running more of the real
design found no defect: D25, D26 and D27 each came out of this same
exercise. After them, Ictus runs picorv32 correctly across the complete
base instruction set -- every ALU operation, both shift directions, every
branch condition, byte/halfword/word loads and stores at every offset,
`jal`/`jalr`, `lui`/`auipc`.

**How the programs are built.** picorv32's firmware calls each test as a
function; `bench/isa/start.S` is a two-instruction stub that jumps into
one test and executes `ebreak` when it returns, so every run -- passing
or failing -- ends with picorv32's `trap` output high, and the characters
the test printed say which. `bench/isa/link.ld` lays code and data out
contiguously from address 0, so the image loads with a plain
`$readmemh`. `bench/isa/build.sh` builds all 37 for picorv32's *default*
configuration (`-march=rv32i`), excluding the multiply, divide and
remainder tests, which need `ENABLE_MUL`/`ENABLE_DIV`. The images are
committed: regenerating them needs the toolchain, running them doesn't.

**Three checks per program, in order.** First, that Icarus itself printed
`<name>..OK` -- a reference run that fails its own test gives nothing
trustworthy to compare against, and is reported as that rather than as an
Ictus failure. Then every traced output, stopping at the first mismatch
(after it, Ictus is being fed responses to requests it didn't make). Then
the register file, which the ports can't vouch for (D25).

**Two testbench details that would each have produced a wrong
reference.** The memory honours byte strobes: the `sb` and `sh` tests
fail against a whole-word memory *in Icarus*, and the earlier testbench
didn't need strobes because its program only stored words. And the
run-until-trap loop is written `trap !== 1'b1`, not `!trap`: before reset
takes effect `trap` is `x`, `!x` is `x`, and a `while` on an `x`
condition doesn't execute -- the first version ended every simulation at
time zero after four cycles.

**A clean pass has to be earned, so this was checked by breaking it.**
The first attempt was a bad check, and the reason is instructive: making
arithmetic right shift logical changed *nothing*, because a `$signed`
operand arrives sign-extended across all 64 bits (D16), so a logical
64-bit right shift of up to 32 places still pulls in copies of the sign
bit, and the low 32 bits come out identical. The difference lives only in
bits that are masked off. That is D20's reasoning working as designed --
arithmetic shift is correct *because* of the sign extension -- but it
meant the perturbation proved nothing. The second attempt made signed
comparison unsigned, which does change mixed-sign results: exactly `bge`,
`blt`, `slt` and `slti` failed, each at its first diverging cycle, while
their unsigned counterparts (`bgeu`, `bltu`, `sltu`, `sltiu`) correctly
still passed. The test is both sensitive and specific.

**Cost**: about 13 seconds in a debug build, almost all of it Ictus
ticking. picorv32 is lowered once and shared across all 37 runs, since
lowering (about 3 seconds) would otherwise dominate.

**Next.** Every remaining step needs something new rather than more of
the same. The cheapest and most directly useful is **top-level parameter
overrides**: picorv32's other single-module configurations --
`BARREL_SHIFTER`, `TWO_CYCLE_ALU`, `TWO_CYCLE_COMPARE`,
`ENABLE_REGS_DUALPORT=0` -- select the *other* branches of the
`generate if`s D27 just made real, and the same 37 programs could then
run against each configuration. Module instantiation is the larger step,
unlocking the multiply and divide tests and the prebuilt firmware.
Cranelift codegen now has the correctness baseline D12 required.

## D29 — Top-level parameter overrides, and measuring what a test can see

**Decision**: `ictus_frontend_verilog::lower_file_with_parameters(path,
&[(name, value)])` overrides a module's top-level parameters -- what
Icarus's `-P` and Verilator's `-G` do -- and the picorv32 instruction
tests now run against four configurations of the core instead of one.
`lower_file` is the same call with no overrides.

**Semantics.** An override replaces a parameter's default at the point the
parameter is resolved, in source order, so everything after it that
depends on it follows: a later parameter or `localparam` computed from
it, a packed range sized by it, and which branch of every `generate if`
is selected. The default expression isn't evaluated at all when
overridden, as in Verilog. Rejected, each with an error naming the
problem: a name that isn't a parameter (a typo would otherwise run the
default configuration and look like success); a `localparam`, which
Verilog forbids overriding; a value wider than the parameter's declared
width; and the same name twice. The width check is a deliberate
departure from Icarus, which silently truncates -- `BARREL_SHIFTER=2` on
a `[0:0]` parameter gives 0 there -- and a configuration flag that
silently means something else is not a reasonable thing to reproduce.

**Driving Icarus with the same configuration was the awkward part.**
`-P` only reaches *root* modules, and picorv32 is instantiated inside the
testbench. Hardcoding picorv32's parameters into the testbench would
duplicate its defaults, and a drift between the copies would make the
"default" run quietly not be one. Instead the test writes a small module
of hierarchical `defparam`s from the same list it hands Ictus, and
compiles it alongside the testbench -- one source of truth for what the
configuration is.

**The configurations**, grouped the way picorv32 is configured in
practice so four runs cover nine parameters: *default*; *fast*
(`BARREL_SHIFTER`, `TWO_CYCLE_ALU`, `TWO_CYCLE_COMPARE`); *small* (one
register-file read port, one-bit-per-cycle shifts, no counters, no
misalignment trap); and *compressed* (`COMPRESSED_ISA`,
`LATCHED_MEM_RDATA`). Every configuration's own reference run was checked
first. `ENABLE_REGS_16_31=0` was excluded because it fails 36 of 37 in
Icarus -- the tests use registers up to x28, which that configuration
removes -- which is the reference check doing its job. `fast` is the first
run in which the clocked branch of `generate if (TWO_CYCLE_ALU)` -- the
one D27 found being lowered alongside the other -- is the live one.

**All 148 runs pass**, about 200,000 cycles. Again no Ictus defect.

**The more useful result is about the test, not the simulator.** Two
questions about a configuration are easy to conflate. *Does it exercise
different logic?* Yes, whenever the override is applied: that logic runs
in Ictus and must match Icarus. *Would the test notice if Ictus silently
ignored the override?* Only if the override changes the bus trace --
and that was measured rather than assumed, one parameter at a time, by
diffing Icarus's traces against the default's: `BARREL_SHIFTER` and
`TWO_STAGE_SHIFT` change 7 of 37 programs, `ENABLE_REGS_DUALPORT` 3,
`TWO_CYCLE_ALU` 1, and `TWO_CYCLE_COMPARE`, the counters,
`CATCH_MISALIGN`, `LATCHED_MEM_RDATA` and `COMPRESSED_ISA` none at all.

Then the check that matters: drop every override on the Ictus side only.
`fast` failed all 37, `small` 7 -- and **`compressed` passed.** On plain
rv32i programs a picorv32 with `COMPRESSED_ISA` behaves identically to
one without, so its compressed-instruction decoder -- a large, intricate
part of the core -- never ran, and the test could not have told whether
Ictus applied the override. The fix was to give it programs that need
it: `bench/isa/build.sh` now also assembles the same tests for `rv32ic`,
where about half the instructions come out 16-bit and 32-bit ones land on
2-byte boundaries. Those fail 35 of 37 in Icarus on a core *without*
`COMPRESSED_ISA`, pass all 37 with it, and pass all 37 in Ictus. Rerun
with Ictus dropping its overrides, `compressed` now fails all 37.

That the override *mechanism* works is established directly, not
inferred from the ISA runs: the frontend's own tests check that a port
width and a derived `localparam` follow an override, that overriding the
conditions of three `generate if`s swaps all three branches, that
picorv32 with `TWO_CYCLE_ALU=1` has its ALU in a clocked process and no
combinational one, and each rejection.

**Cost**: the four configurations are separate test functions so cargo
runs them in parallel -- about 22 seconds of wall time, 80 of CPU. Each
builds its Icarus testbench in its own directory for the same reason.

**Next**: module instantiation. It is now the largest gap by a distance,
and the thing between Ictus and the multiply/divide tests, picorv32's own
firmware, and running a testbench directly rather than replaying one.

## D30 — Module instantiation by flattening; and a width defect it surfaced

**Decision**: a module instance is **flattened into its parent at lowering
time**. Each instantiated module is lowered on its own (recursively, with
the parameter values the instantiation gives it) and then merged in: its
signals become the parent's, named `<instance>.<name>`, its lowered IR is
renumbered to match, and its ports are connected. The IR and the kernel
don't change at all -- the result is one flat `Module`, as every design
has been until now.

**Why flatten rather than keep a hierarchy in the kernel.** A hierarchy
would mean a netlist model in the kernel -- instances, port bindings,
signals owned by scopes -- and every kernel feature so far (fixpoint
settling, the one-driver rule, the commit phase) would have to be
re-thought across instance boundaries. Flattening keeps all of that
exactly as it is and puts the whole cost in one lowering step. It is
what Verilator does, it is the natural shape for D12's baseline
interpreter, and it suits the mixed-language goal (D11): a VHDL entity
instantiated from Verilog would flatten into the same IR. What it gives
up is sharing between repeated instances (memory, compile time) and the
hierarchy as a runtime structure -- the names keep the hierarchy for
anything that needs to display it, like a waveform viewer. If
thousand-instance designs make the memory cost real, that is the point to
revisit, and the compiled kernel is the natural place to do it.

**Ports: aliased when possible, assigned otherwise.** A port connected
straight to a whole parent signal of the same width is *aliased*: the
child's references are pointed at the parent's signal, and no copy
exists. Everything else goes through a continuous assignment, which is
how IEEE 1800 describes a port connection anyway and which gives
Verilog's width coercion for free, since the kernel masks every write to
its target's width: an input connected to an expression (`.in(a + b)`)
or to a signal of another width becomes a copy of the port driven by the
connection; an output connected to a signal of another width becomes a
copy with `signal = port` in the parent.

The clock is why aliasing matters rather than being an optimization.
Connecting `.clk(clk)` by copying would give the child's `always
@(posedge clk)` a clock that is a *different signal* from the parent's,
fed by an assignment. The kernel has always ticked every clocked process
together, assuming one clock; with copies, "the design has one clock"
would no longer be something the lowered module could be checked for.
With aliasing it is exact, so it now *is* checked: a design whose
clocked processes use more than one clock signal -- a child clocked by
`clk & en`, say -- is rejected, where before instances it could only have
been ticked together and been silently wrong.

**Also newly checked**: a module's input port driven from inside it.
Flattening makes this reachable -- a child's output wired onto the
parent's own input port is aliased onto it -- and it would otherwise be
silently overwritten by whatever drives the input. The one-driver check
(D27) now runs after flattening at each level, so a conflict across an
instance boundary is caught like any other.

**Supported and rejected.** Named connections and named parameter values
-- `child #(.P(v)) u (.port(expr), ...)`, several instances per statement
-- which is what picorv32 and most real RTL use. Parameter values are
expressions in the *parent's* scope, folded to constants and passed down
through D29's override mechanism, so a child's `generate if` selection
follows its instantiation exactly as a top-level override would.
Rejected, each with a specific error: positional ports and parameters
(they depend on declaration order, a real source of bugs, and nothing
needs them yet); `.*` and implicit `.port`; instance arrays; an unknown
module, port or duplicate; an output connected to a part-select or
concatenation (that would need a continuous assignment to part of a
signal, which v1 doesn't support); a module that instantiates itself;
and an *unconnected input*, which Verilog leaves floating at `z` -- not
representable in 2-state, and far more often a mistake than a choice.

Renumbering the child's IR uses new `remap_signals` methods on `Expr` and
`Stmt`, written as exhaustive matches with no wildcard arm: a future
variant that carries a signal must fail to compile there until it is
handled, rather than being silently left pointing at the wrong signal.

**Unary minus came along**, because picorv32's divider needs it: `-x` is
lowered as `0 - x` with the zero at the operand's width, sharing
subtraction's evaluation exactly (unary `+` is the identity).

**Result: picorv32 with `ENABLE_DIV=1` runs its divide and remainder
tests.** The divider, `picorv32_pcpi_div`, is a separate module picorv32
instantiates -- the first instance in a real design Ictus has flattened,
doing real work over the co-processor interface across many cycles. All
four programs (`div`, `divu`, `rem`, `remu`, rv32im images from a third
image set) match Icarus on every port, every cycle, and the register
file. Checked by breaking it: making unary minus compute `x - 0` failed
exactly `div` and `rem` -- the signed ones, which negate -- while `divu`
and `remu` passed. Separately, a differential test on a three-level
fixture (`instance_test.v`) checks aliasing, per-instance state, an
expression-driven input, a truncating narrow output and a parameter
passed down two levels against Icarus's own handling of the hierarchy.

The ISA test's floor on how many points were comparable was an absolute
count calibrated for 37 programs, and the four-program divider run
failed it while agreeing with Icarus on every point. It is now a
proportion (at least 98%; measured at 99.1-99.5%), which is what it was
meant to guard in the first place.

**A correction to D29.** It said instantiation would let Ictus "run a
testbench directly rather than replaying one". It doesn't: a testbench
needs `initial` blocks, delays, event waits and `$display`, none of which
Ictus models. The trace-replay harness stays, and the comments that gave
instantiation as the reason for it now give the real one.

**A defect this surfaced, which is now the most important open item.**
Because unary minus is lowered as subtraction, it inherits how the kernel
evaluates subtraction: on a 64-bit word, relying on masking at the final
write. That is correct for operators whose low bits depend only on their
operands' low bits -- add, subtract, multiply, left shift, bitwise -- and
**wrong for operators that look at high bits**, applied to an arithmetic
result that wrapped. Probed against Icarus with 8-bit `a = 3`, `b = 5`:

| expression         | Icarus | Ictus |
|--------------------|--------|-------|
| `(a - b) >> 1`     | 127    | 255   |
| `(a - b) < 8'hFF`  | 1      | 0     |
| `(-a) >> 1`        | 126    | 254   |
| `(a - b) == 8'hFE` | 1      | 0     |

Verilog evaluates the subtraction at 8 bits (IEEE 1800 §11.6,
context-determined width), so the wrap happens *before* the shift or
comparison. This is not new with unary minus -- subtraction, addition and
multiplication have always behaved this way -- and nothing in the suite
caught it, picorv32 included, because its arithmetic results go straight
into registers where the final mask makes them right. It is exactly the
kind of silent wrong answer this project exists to avoid, so it is kept
as a runnable reproduction, `differential_width_context.rs`, marked
`#[ignore]` with the reason, rather than only described here. Fixing it
means implementing Verilog's context-determined expression widths
properly, which needs its own design pass: it interacts with `$signed`
sign extension, concatenation's self-determined operands, the ternary
operator and assignment width.
