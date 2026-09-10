# Ictus

A Rust HDL simulator aimed at beating Verilator on the thing that actually
matters for adoption: the edit-simulate loop. Not a bid for QuestaSim/Vivado
feature parity (no UVM, no full SVA, no gate-level SDF in the near term) —
the wedge is raw performance plus usability, not a verification-suite
checklist.

## The bet

- **JIT instead of AOT-via-system-compiler.** Verilator generates C++ and
  shells out to GCC/Clang; for large designs that recompile step dominates
  iteration time. Ictus compiles the elaborated design straight to native
  code in-process via Cranelift, so both codegen and turnaround stay fast.
- **Native SystemVerilog testbenches.** Verilator requires a C++/SystemC
  harness around the DUT. Ictus runs the synthesizable RTL through the
  compiled engine and the non-synthesizable testbench layer (`initial`,
  `#delay`, `fork`/`join`) through a lighter interpreted path, so a plain SV
  testbench just works.

## Architecture

1. **Frontend per language** (Verilog first, via `sv-parser`; VHDL later via
   `vhdl_lang`) parses into its own AST, then lowers to a shared `ictus-ir`.
2. **Elaboration** resolves generics/parameters, generate blocks, instance
   hierarchy, and cross-language port binding.
3. **Kernel**: cycle-based, not classic delta-cycle event-driven. The
   dataflow graph is topologically sorted and evaluated once per relevant
   clock edge — a deliberate accuracy/throughput trade for synchronous RTL.
   A small classic event-driven kernel handles the testbench layer
   alongside it.
4. **Signal representation**: 2-state, bit-packed, SIMD-friendly by default;
   4-state (X/Z) is opt-in per-signal/module, not the default cost.
5. **Parallelism**: static partitioning of the dataflow graph into
   independent clusters at elaboration time (Verilator's MTask approach),
   not fine-grained per-process scheduling.
6. **Waveforms**: FST via a clean-room writer (or `wellen`, BSD-3) — not
   ported from gtkwave's GPLv2 core. Debug UI integrates with
   [Surfer](https://gitlab.com/surfer-project/surfer).

## Roadmap

- [ ] **Phase 0** — benchmark harness against real open cores (PicoRV32,
      Ibex, ...) before any kernel code, so "faster than Verilator" is a
      measured claim, not an assumption.
- [ ] **Phase 1** — Verilog RTL subset → IR → single-threaded cycle-based
      engine (2-state) via Cranelift JIT.
- [ ] **Phase 2** — static multi-threaded partitioning.
- [ ] **Phase 3** — SystemVerilog RTL constructs (interfaces, packed
      structs/enums; no classes/UVM) + interpreted testbench layer.
- [ ] **Phase 4** — VHDL frontend + mixed-language elaboration.
- [ ] **Phase 5** — FST output + Surfer integration.
- [ ] **Phase 6 (stretch)** — optional rustc/LLVM AOT path for max-throughput
      CI runs, opt-in 4-state fidelity, gate-level/SDF.

Explicitly out of scope unless the strategy changes: UVM, constrained-random,
functional coverage, full SVA, IP encryption.

## Status

Early scaffold. Crate layout exists; no parsing, elaboration, or simulation
logic yet.

## Workspace layout

- `crates/ictus-ir` — shared IR consumed by every frontend and the kernel.
- `crates/ictus-frontend-verilog` — Verilog parsing (`sv-parser`) → IR lowering.
- `crates/ictus-kernel` — the cycle-based execution engine.
- `crates/ictus-cli` — the `ictus` binary.

## Building

Requires a Rust toolchain (stable). From the workspace root:

```
cargo build
```

See [docs/development.md](docs/development.md) for this project's actual
toolchain setup.

## Documentation

The [docs/](docs/) folder is the long-form working memory for this project
— architecture rationale, a decision log with alternatives considered and
rejected, the detailed phase-by-phase roadmap, and a domain glossary.
Anyone (or any agent) picking this project up cold should start at
[docs/README.md](docs/README.md).
