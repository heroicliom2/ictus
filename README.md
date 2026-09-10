# Ictus

Ictus is an open-source HDL simulator for **mixed-language RTL
simulation** — Verilog, SystemVerilog, and VHDL in the same design,
natively, without wrapping the design or its testbench in C/C++ or
SystemC models to make it work.

*(This README uses a few software-engineering terms that aren't standard
EE vocabulary — "JIT," "Cranelift," "dataflow graph," and so on. Every one
of them is explained in full in [docs/glossary.md](docs/glossary.md); this
file only gives the short version inline.)*

## Why this exists

There isn't a real open-source simulator today that does this well. Icarus
Verilog is Verilog-only. GHDL is VHDL-only. Verilator is fast but 2-state,
RTL-focused, and requires a C++/SystemC harness even to run a testbench.
Mixed VHDL/Verilog verification — routine in real chip and FPGA projects —
is effectively proprietary-tool-only territory: QuestaSim, Xcelium, Vivado
Simulator. Ictus is built to close that gap in the open.

## Three pillars

Every design and scope decision is checked against all three of these —
none of them gets sacrificed for another:

1. **Speed and a lightweight footprint.** A cycle-based execution kernel
   — re-evaluate the whole design once per clock edge, in dependency
   order, rather than processing a fine-grained queue of individual signal
   change events the way classic simulators do — paired with Cranelift, a
   Rust code generator that compiles the design directly into real CPU
   instructions *while the tool is running* ("JIT," Just-In-Time
   compilation), instead of writing out C++ and separately invoking a
   full C++ compiler the way Verilator does. That collapses what's
   normally a slow two-step build into one fast in-process step, so the
   edit-simulate loop stays quick without a second compiler toolchain in
   the way.
2. **True mixed-language support, natively.** Verilog/SystemVerilog and
   VHDL in one simulation, with cross-language elaboration and port
   binding as a first-class concern — and no requirement to drop into
   C/C++ or SystemC models, for the design *or* the testbench, to make any
   of it work.
3. **A real path to full verification-suite capability.** UVM-class
   methodology, SVA, functional/code coverage — genuine long-term goals,
   not rejected scope. They're deliberately sequenced behind getting
   pillars 1 and 2 right first, and only ever added in ways that don't
   compromise them.

## Current scope

Early stage, RTL simulation first. Verilog now, SystemVerilog RTL
constructs and VHDL next; verification-suite features (UVM/SVA/coverage)
come later, once the mixed-language RTL foundation and the kernel are
solid — see [docs/roadmap.md](docs/roadmap.md) for the phase-by-phase plan
and [docs/decisions.md](docs/decisions.md) for the reasoning behind that
sequencing.

## Status

Phase 1 in progress (see docs/roadmap.md). A narrow Verilog subset (single
module, any number of clocked `always @(posedge clk)` blocks and
continuous `assign`s, `if`/`else`, non-blocking assignment, internal
`wire`/`reg` declarations, the operators `+ & | ^ == != < <= > >= && !`,
decimal/hex/binary literals) parses, lowers to `ictus-ir`, and runs
correctly on a tree-walking interpreter — checked with cycle-for-cycle
differential tests against Icarus Verilog. Cranelift JIT codegen, `case`,
bit-select/concatenation, and the phase 0 benchmark designs (picorv32
first) are all still ahead of where this stands today.

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
