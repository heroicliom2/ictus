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
