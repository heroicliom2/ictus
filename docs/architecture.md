# Architecture

*This doc uses software/compiler-engineering terms that aren't standard EE
vocabulary (JIT, Cranelift, dataflow graph, topological sort, static vs.
dynamic scheduling, SIMD, bit-packing, and the various software licenses).
Each gets a short explanation inline on first use below; the full-detail
version of every one of them lives in [glossary.md](glossary.md).*

## The strategic bet

Ictus exists to fill a real gap: there is no real open-source simulator
today that does **mixed-language RTL simulation** — Verilog/SystemVerilog
and VHDL in the same design — without forcing users to drop into C/C++ or
SystemC models to glue it together. Icarus Verilog is Verilog-only. GHDL is
VHDL-only. Verilator is fast but 2-state, RTL-focused, and requires a
C++/SystemC harness even for the testbench layer. Nothing open-source
covers what QuestaSim/Xcelium/Vivado Simulator cover for mixed-language
verification. That's the gap.

The long-term goal is a genuine open-source replacement for people
currently locked into QuestaSim-class proprietary tools — including,
eventually, real verification-suite capability (UVM-class methodology,
SVA, coverage). That is a real goal, not rejected scope (see
[decisions.md](decisions.md), D11, which supersedes the earlier framing in
D1/D10). It is *sequenced* deliberately, never rushed in at the cost of the
other two pillars below.

Every design and scope decision in this project gets checked against three
pillars, none of which is disposable in favor of another:

1. **Speed / lightweight execution.**
2. **True mixed-language support, natively** — no C/C++/SystemC models
   required to make Verilog/SV and VHDL work together, or to run a
   testbench.
3. **A genuine long-term path to full verification-suite capability**,
   added only when and how it doesn't compromise pillars 1 and 2.

Verilator is a useful technical reference point for pillar 1 specifically
(it proved cycle-based compiled simulation beats event-interpreted
simulation by an order of magnitude, and its architecture choices are
worth learning from), not a competitor Ictus is defined in opposition to.
The comparisons below are about borrowing or deliberately diverging from
proven techniques, not about "winning."

Two structural choices that address pillars 1 and 2 together, both baked
into the architecture rather than bolted on later:

1. **JIT instead of AOT-via-system-compiler.** Two ways to turn a design
   into something that actually runs: compile it fully as a separate step
   before simulation starts ("AOT," Ahead-Of-Time — what Verilator does:
   it generates C++ source and you must separately invoke a full C++
   compiler like GCC or Clang before you can simulate anything), or
   compile it to machine code on the spot, inside the same running
   process, right as it's needed ("JIT," Just-In-Time). For large designs,
   Verilator's separate AOT compile step can take minutes and dominates
   the actual edit-simulate loop. Ictus compiles the elaborated design
   straight to native machine code in-process via **Cranelift** — a
   Rust-native code generator (originally built for, and still used by,
   the Wasmtime WebAssembly runtime) that prioritizes fast compilation
   over the absolute best-possible output — getting fast codegen *and*
   fast turnaround without a second toolchain in the loop.
2. **Native SystemVerilog testbench execution.** Verilator requires a
   C++/SystemC harness around the DUT — you cannot point it at a plain SV
   testbench. Ictus runs synthesizable RTL through the compiled engine and
   the non-synthesizable layer (`initial` blocks, `#delay`, `fork`/`join`,
   class-free testbench code) through a lighter interpreted path, so a
   normal SV testbench works without a rewrite.

## Pipeline

```
source (.v/.sv, .vhd)
   │
   ▼
per-language frontend  ──►  parses into its own AST
   │
   ▼
lowering                ──►  common IR (ictus-ir)
   │
   ▼
elaboration              ──►  resolve generics/parameters, generate blocks,
   │                          instance hierarchy, cross-language port binding
   ▼
kernel                   ──►  cycle-based compiled engine (RTL) +
   │                          interpreted event kernel (testbench layer)
   ▼
waveform output (FST) / Surfer integration
```

("Lowering" in the diagram above means translating from a higher-level
representation into a simpler, more explicit one closer to what the next
stage needs — see glossary.md.)

A frontend's job is to turn source text into an **AST** (Abstract Syntax
Tree — a tree data structure representing the code's grammatical
structure, e.g. "this is a module containing these statements, this
statement is an assignment with this left side and this right side"; it's
the standard first output of any compiler/parser, not something specific
to this project). "Lexing" is the step just before that: splitting raw
source text into meaningful chunks (keywords, identifiers, numbers,
punctuation) before the grammatical structure is assembled from them.

### Frontends

- **Verilog**: `sv-parser` (IEEE 1800-2017, MIT/Apache-2.0). This gives
  syntax only — lexing/parsing/preprocessing, producing an AST. It does
  **not** give type checking, parameter/generate elaboration, or name
  resolution; all of that is still work this project owns. Don't let the
  crate's existence make elaboration feel smaller than it is.
- **VHDL** (phase 4): `vhdl_lang`, the analyzer core of the VHDL-LS project
  (actively maintained under the VHDL-LS GitHub org, not a stale solo
  project — see decisions.md D8). It's built for LSP-style semantic
  analysis (LSP = Language Server Protocol, the standard editors like
  VS Code use to get live diagnostics, go-to-definition, etc. from a
  language-aware backend); how much of its internal AST/semantic model is
  reusable for elaboration-for-simulation vs. LSP-specific is an open
  question to resolve when phase 4 starts.
- Editor tooling (`tree-sitter-verilog`/`tree-sitter-systemverilog`) is
  noted as useful for IDE features later, not a simulation-grade AST source.
  Not a current dependency.

### Intermediate representation (`ictus-ir`)

Shared elaborated representation all frontends lower into; elaboration and
the kernel only ever operate on this, never on a frontend's own AST. Design
is not yet fleshed out beyond a placeholder type — real shape (module
hierarchy, nets/signals, comb/seq process nodes represented as a
**dataflow graph** — a graph where each node is an operation like "this
always-block" and each edge is a value flowing into something that depends
on it, rather than a flat list of statements) is phase 1 work. Think of it
as similar in spirit to UHDM, but designed for simulation throughput, not
static analysis.

### Elaboration

Resolves generics/parameters, generate blocks, instance hierarchy, and —
the hardest part of mixed-language support — cross-language port binding
(signal type/width compatibility checking across a VHDL/Verilog instance
boundary). This is where most of the real semantic complexity lives, not in
either frontend.

### Kernel: cycle-based, not classic event-driven

The primary engine is **cycle-based**: the dataflow graph is
**topologically sorted** — ordered so that every node comes after
everything it depends on, a standard graph algorithm, nothing
project-specific — and combinational/clocked logic
(`always_comb`/`always_ff`) is evaluated once per relevant clock edge in
that order, rather than processing a fine-grained delta-cycle event queue. This is deliberately what makes
Verilator fast, and it's an explicit accuracy trade — exact sub-cycle
delta-cycle timing fidelity is given up in exchange for throughput on
synchronous RTL.

A **separate, smaller classic event-driven kernel** runs alongside it,
scoped to the non-synthesizable testbench layer only (`#delay` stimulus,
asynchronous glue, `fork`/`join`), so timing-sensitive testbench code still
behaves correctly. Two tiers, not one kernel trying to do both jobs.

If SVA assertions ever come into scope, note that IEEE 1800 defines more
scheduling regions than a naive "active/inactive/postponed" model —
preponed, active, inactive, NBA, observed, reactive, re-inactive, re-NBA,
postponed. The extra regions exist specifically for assertions and program
blocks. Retrofitting the full region set later means touching the kernel
core, so if assertions are ever prioritized, model the real region set from
the start of that work.

### Signal representation

2-state by default, **bit-packed** (many individual 1-bit signals stored
packed together inside larger machine words — e.g. 64 signal bits inside
one 64-bit integer — instead of one byte or object per bit, which is far
more memory- and cache-efficient) and **SIMD-friendly** (laid out so the
CPU can apply one instruction to many signal bits at once, instead of one
instruction per bit — SIMD = Single Instruction, Multiple Data). This is
Verilator's own hard-won performance lesson: 4-state/X-Z tracking is
expensive and mostly unnecessary for verified RTL. 4-state semantics (X/Z)
are opt-in per-signal or per-module, for reset/uninitialized-read checking,
not the default representation.

### Parallelism

**Static partitioning at elaboration time**: split the dataflow graph into
independent clusters once, before simulation starts, and permanently
assign each cluster to a thread — no runtime decision-making about which
thread runs what ("dynamic scheduling") on a per-event basis. This mirrors
Verilator's proven `--threads`/MTask approach (MTask being Verilator's name
for one of these statically-assigned parallel work units). A dynamic
alternative called **work-stealing** — where idle threads grab tasks from
busy threads' queues at runtime, via the Rust `rayon` library — was
considered and rejected for the core kernel: individual HDL simulation
events are too small and numerous for that runtime bookkeeping to pay off.
See decisions.md D5.

### Waveform output

FST (GTKWave/Surfer's compressed format), not plain VCD, for size/speed.
**Do not port code from gtkwave's C core** — its licensing is GPLv2
("copyleft": code built from GPL sources generally has to be released
under the GPL too, which would force a license change on this whole
project — see glossary.md, Licensing terms) with open ambiguity even in
gtkwave's own issue tracker about whether `fstapi.c`/libfst specifically is
GPL or MIT (a permissive license — usable in any project, no license
change forced), plus an optional LGPL-gated (a weaker, partial copyleft)
Judy-array path. See decisions.md D7 for the full reasoning and the safe
alternatives (`wellen`, BSD-3-licensed — another permissive license — for
reading; a clean-room writer or `libfstwriter`, confirmed MIT, if a writer
is needed).

### Debug UI

Integrate with [Surfer](https://gitlab.com/surfer-project/surfer) (Rust,
egui-based, EUPL-licensed — the EU's own copyleft license, roughly
GPL-like in effect, but this is a separate program Ictus talks to, not
code copied into this project, so it doesn't carry the same
license-contamination risk as D7's gtkwave concern — actively developed)
rather than building a waveform viewer from scratch. Surfer's own waveform I/O layer is `wellen`,
which is the same crate this project should use for FST/VCD handling.

## Deferred, not rejected (near-term)

UVM, constrained-random, functional/code coverage, full SVA, gate-level
timing/SDF back-annotation, IEEE P1735 IP encryption are not part of the
current phased roadmap (phases 0-5). These are real long-term goals (pillar
3, decisions.md D11), deliberately sequenced behind getting mixed-language
RTL simulation and the speed/lightweight kernel right first (pillars 1-2).
Each would roughly 10x scope if pulled forward, and pulling one forward
before it can be done without compromising speed or the native
multi-language story would be the wrong tradeoff — see decisions.md D10/D11.

## Validation strategy

A simulator nobody can prove is correct is a toy. Plan:

- **Benchmark harness before kernel code** (phase 0): 3-5 real open cores
  (PicoRV32, Ibex, CV32E40P/Rocket) as the perf regression suite, so pillar
  1 (speed/lightweight) is a measured, continuously tracked property from
  day one, not an assumption made once and never checked again.
- **Correctness**: differential testing against Icarus Verilog and
  Verilator, plus adopting an existing conformance suite (chipsalliance's
  `sv-tests`) rather than inventing test cases ad hoc.
