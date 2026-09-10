# Decisions

Lightweight ADR log. Each entry: the decision, the alternatives considered,
and why — so a fresh agent doesn't re-litigate settled questions or redo
rejected work. Newest at the bottom. Add an entry whenever a real fork in
the road gets resolved in conversation; don't bother for routine
implementation choices that don't foreclose an alternative.

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

**Decision**: compile the elaborated design to native code in-process via
Cranelift at load time, rather than generating C++/Rust source and shelling
out to a system compiler (Verilator's model).

**Alternatives considered**: generate C++ and invoke GCC/Clang (copy
Verilator's approach); generate Rust and invoke `rustc`.

**Why**: for large designs, Verilator's recompile step can take minutes and
dominates the actual edit-simulate loop — this is arguably Verilator's real
adoption friction, more than raw runtime throughput. Cranelift (pure Rust,
wasmtime's JIT backend) gives fast codegen and fast turnaround without a
second toolchain dependency. An optional AOT path (emit to `rustc`/LLVM) is
kept as a phase-6 stretch goal for max-throughput CI runs where compile time
matters less — mirrors `cargo run` vs `cargo run --release`.

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

**Alternatives considered**: rayon-based work-stealing across independent
processes at runtime (part of the original starting proposal).

**Why**: fine-grained parallel event-driven RTL simulation is a known-hard
research problem — event granularity is typically too fine for
thread-per-process to pay off once synchronization overhead and
shared-signal fan-out dependencies are accounted for. Static partitioning is
the proven version of the same intuition and is what Verilator actually
ships.

## D6 — Signal representation: 2-state default, 4-state opt-in

**Decision**: bit-packed, SIMD-friendly 2-state vectors by default. 4-state
(X/Z) tracking is opt-in per-signal or per-module.

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

**Why**: gtkwave itself is GPLv2, and there's open, unresolved ambiguity in
gtkwave's own issue tracker (gtkwave/gtkwave#309) about whether
`fstapi.c`/libfst specifically is GPL or MIT, plus an optional
LGPL-gated Judy-array code path. Porting from it risks pulling GPL-encumbered
code into what should be a permissively-licensed (MIT/Apache-2.0) project.
`wellen` sidesteps this entirely for reading and is already the dependency
Surfer integration (D-later, architecture.md) needs anyway.

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
