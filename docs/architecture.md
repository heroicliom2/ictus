# Architecture

## The strategic bet

Ictus is not aiming for feature parity with QuestaSim or Vivado Simulator —
that's a decade-plus, large-team undertaking (full IEEE 1800/1076
compliance, UVM, SVA, functional/code coverage, mixed-signal, gate-level
timing with SDF, IEEE P1735 IP encryption). Matching that checklist was
considered and explicitly rejected as the near-term goal (see
[decisions.md](decisions.md), D1).

The chosen wedge is **performance**: beat Verilator on the thing that
actually drives adoption, the edit-simulate loop, in a scope limited to RTL
simulation. Verilator itself never matched VCS/Questa on verification
completeness and won real adoption anyway by being dramatically faster and
free. The target here is to out-Verilator Verilator on its own axis, plus
close its biggest usability gap (see below).

Two structural advantages over Verilator, both baked into the architecture
rather than bolted on later:

1. **JIT instead of AOT-via-system-compiler.** Verilator generates C++ and
   shells out to GCC/Clang; for large designs that recompile step can take
   minutes and dominates the edit-simulate loop. Ictus compiles the
   elaborated design straight to native code in-process via **Cranelift**
   (pure Rust, the JIT backend behind wasmtime), getting fast codegen *and*
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

### Frontends

- **Verilog**: `sv-parser` (IEEE 1800-2017, MIT/Apache-2.0). This gives
  syntax only — lexing/parsing/preprocessing. It does **not** give type
  checking, parameter/generate elaboration, or name resolution; all of that
  is still work this project owns. Don't let the crate's existence make
  elaboration feel smaller than it is.
- **VHDL** (phase 4): `vhdl_lang`, the analyzer core of the VHDL-LS project
  (actively maintained under the VHDL-LS GitHub org, not a stale solo
  project — see decisions.md D8). It's built for LSP-style semantic
  analysis (diagnostics, go-to-definition); how much of its internal
  AST/semantic model is reusable for elaboration-for-simulation vs.
  LSP-specific is an open question to resolve when phase 4 starts.
- Editor tooling (`tree-sitter-verilog`/`tree-sitter-systemverilog`) is
  noted as useful for IDE features later, not a simulation-grade AST source.
  Not a current dependency.

### Intermediate representation (`ictus-ir`)

Shared elaborated representation all frontends lower into; elaboration and
the kernel only ever operate on this, never on a frontend's own AST. Design
is not yet fleshed out beyond a placeholder type — real shape (module
hierarchy, nets/signals, comb/seq process nodes as a dataflow graph) is
phase 1 work. Think of it as similar in spirit to UHDM, but designed for
simulation throughput, not static analysis.

### Elaboration

Resolves generics/parameters, generate blocks, instance hierarchy, and —
the hardest part of mixed-language support — cross-language port binding
(signal type/width compatibility checking across a VHDL/Verilog instance
boundary). This is where most of the real semantic complexity lives, not in
either frontend.

### Kernel: cycle-based, not classic event-driven

The primary engine is **cycle-based**: the dataflow graph is topologically
sorted and combinational/clocked logic (`always_comb`/`always_ff`) is
evaluated once per relevant clock edge, rather than processing a
fine-grained delta-cycle event queue. This is deliberately what makes
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

2-state by default, bit-packed and SIMD-friendly — this is Verilator's own
hard-won performance lesson (4-state/X-Z tracking is expensive and mostly
unnecessary for verified RTL). 4-state semantics (X/Z) are opt-in per-signal
or per-module, for reset/uninitialized-read checking, not the default
representation.

### Parallelism

**Static partitioning at elaboration time**: split the dataflow graph into
independent clusters once, assign to threads, no dynamic scheduling
overhead per event. This mirrors Verilator's proven `--threads`/MTask
approach. Fine-grained "rayon per process" was considered and rejected —
see decisions.md D5.

### Waveform output

FST (GTKWave/Surfer's compressed format), not plain VCD, for size/speed.
**Do not port code from gtkwave's C core** — its licensing is GPLv2 with
open ambiguity even in gtkwave's own issue tracker about whether
`fstapi.c`/libfst specifically is GPL or MIT, plus an optional
LGPL-gated Judy-array path. See decisions.md D7 for the full reasoning and
the safe alternatives (`wellen`, BSD-3, for reading; a clean-room writer or
`libfstwriter`, confirmed MIT, if a writer is needed).

### Debug UI

Integrate with [Surfer](https://gitlab.com/surfer-project/surfer) (Rust,
egui-based, EUPL-licensed, actively developed) rather than building a
waveform viewer from scratch. Surfer's own waveform I/O layer is `wellen`,
which is the same crate this project should use for FST/VCD handling.

## Explicitly out of scope (near-term)

UVM, constrained-random, functional/code coverage, full SVA, gate-level
timing/SDF back-annotation, IEEE P1735 IP encryption. Each of these would
roughly 10x project scope without moving the "is it faster" needle, which
is the entire point of the chosen wedge. Revisit only if the performance
niche succeeds and there's a deliberate reason to expand up-market — see
decisions.md D10.

## Validation strategy

A simulator nobody can prove is correct is a toy. Plan:

- **Benchmark harness before kernel code** (phase 0): 3-5 real open cores
  (PicoRV32, Ibex, CV32E40P/Rocket) as the perf regression suite, so
  "faster than Verilator" is a measured, continuously tracked claim from
  day one, not an assumption made once and never checked again.
- **Correctness**: differential testing against Icarus Verilog and
  Verilator, plus adopting an existing conformance suite (chipsalliance's
  `sv-tests`) rather than inventing test cases ad hoc.
