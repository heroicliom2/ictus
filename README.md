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
module, ports that may inherit direction in a list, any number of clocked
`always @(posedge clk)` blocks and continuous `assign`s, `if`/`else`/`else
if`, `case`/`casez`/`casex` with wildcard bits, both non-blocking (`<=`) and
blocking (`=`) assignment,
internal `wire`/`reg` declarations naming one or more signals each,
module parameters *and* `localparam` (resolved to constants at lowering
time, sharing one name table — a `localparam`'s value may reference an
earlier parameter, use the ternary operator, and use arithmetic),
constant/variable bit-select, constant part-select, concatenation
(including replication, `{N{a,b}}`), and the ternary operator on reads
(`x[3]`, `x[i]`, `x[7:0]`, `{a,b}`, `{4{a,b}}`, `c ? a : b`), plus a
*constant* bit-select/part-select as a procedural
assignment target (`x[7:0] <= v;`, with correct read-modify-write
semantics in the kernel — the rest of the signal's bits are left
untouched, and multiple partial writes to the same signal in one clock
edge combine correctly — and the target index may itself reference a
parameter/localparam), the operators
`+ - * << >> >>> & | ^ == != < <= > >= && !`, decimal/hex/binary
literals) parses, lowers to `ictus-ir`, and runs
correctly on a tree-walking interpreter — checked with cycle-for-cycle
differential tests against Icarus Verilog, including an ongoing real
attempt at lowering the actual phase 0 picorv32 benchmark design (not
just hand-written fixtures), which is how most of the gaps just closed
were found. A concatenation of such targets (`{a, b[3:0]} <= v;`) is
also supported, split into one write per part at lowering time. The
`$signed(...)` system function sign-extends a value into a wider
assignment target and marks operands for the signed-aware operators
(arithmetic right shift, signed comparison). A call to a
*provably-empty* task (`some_task;`) is
supported as a true no-op — picorv32's own `` `assert(...) `` macro
relies on exactly this. Unary bitwise complement (`~`) and the reduction
operators (`&`, `|`, `^`, `~&`, `~|`, `~^`/`^~`) are also supported now.
A 4-state `x`/`z` digit in a literal outside a `case`/`casez`/`casex`
item resolves to `0` (matching Verilator's own default X-handling
policy), and a comparison/logical/reduction result (always exactly 1
bit) can be used as a concatenation operand. So can a binary bitwise or
arithmetic result, at the width Verilog gives it — the wider of its two
operands, which means an adder's carry is truncated away exactly as a
real simulator truncates it. Shifts are supported
including the arithmetic right shift (`$signed(x) >>> n`, which really
does replicate the sign bit), as are signed ordering comparisons
(`$signed(a) < $signed(b)`, which picorv32's ALU needs). Array/memory
signals (`reg [31:0] mem [0:31]`, read and written one element at a time
at a runtime index) are supported too — picorv32's register file is
exactly this shape. Blocking assignment (`=`) works alongside `<=` in the
same clocked block, with the timing Verilog defines: a blocking write
lands immediately, so the next statement reads the new value, while a
non-blocking one is still invisible to a later read in the same edge.

The whole of the vendored picorv32.v now lowers through this frontend
cleanly — 225 signals, no error — which is what the running diagnostic
against the real design had been driving toward. Lowering cleanly is not
the same as simulating correctly, and establishing the latter is the next
thing on the roadmap. Compound assignment (`+=` and friends), module
instantiation, and Cranelift JIT codegen are all still ahead of where
this stands today.

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
