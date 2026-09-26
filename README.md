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

Phase 1 in progress (see docs/roadmap.md). A narrow Verilog subset (modules
instantiated by name and flattened into one, ports that may inherit
direction in a list, any number of clocked
`always @(posedge clk)` blocks, combinational `always @*` blocks, and
continuous `assign`s, `if`/`else`/`else
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
Combinational `always @*`/`always_comb` blocks are supported as well,
with the kernel settling all combinational logic to a fixpoint so
declaration order does not affect the result.

The whole of the vendored picorv32.v — a real 32-bit RISC-V CPU core —
lowers through this frontend cleanly and **executes correctly**: run
against a recorded trace from Icarus Verilog, every traced port matches
cycle for cycle and the register file matches at the end, across a
program that does ALU work, a store, a load and a branch.

Getting there found four defects that each let the design lower, run, and
produce plausible output while being wrong — none reachable by reading
the code or by a feature-sized test. Driving an input didn't re-settle
combinational logic. Net declarations carrying an initializer
(`wire x = a + b;`, which is how most of picorv32's combinational logic
is written) were dropped silently. Binary operator precedence was never
applied at all, so the instruction decoder read `addi` as a shift
instruction. And `always @*` blocks were ignored, which left the core
reproducing every bus cycle while writing no registers at all — a
reminder that agreement on a design's ports is not evidence that the
design ran.

A fifth turned up afterwards, and it had been hiding behind correct
results: both branches of every `generate if` were being lowered.
picorv32 got right answers only because its conflicting branches happened
to resolve the right way for its default parameters. `generate if` is now
elaborated against the parameters, module instantiation is rejected
rather than silently dropped, and a signal driven from more than one
place is refused outright.

With that fixed, picorv32's own instruction-test suite runs too: all 37
of its base-ISA programs (the riscv-tests rv32ui suite, about 49,000
cycles) match Icarus on every traced port, every cycle, and on the final
register file. That is the first time running more of the real design
turned up nothing wrong.

Top-level parameters can be overridden too (the equivalent of Icarus's
`-P`), so the same suite also runs against three other configurations of
picorv32 — tuned for speed, tuned for area, and with compressed
instructions — 148 runs in all, every one matching Icarus.

Modules can now instantiate other modules, by name. Each instance is
flattened into its parent when the design is lowered, so the simulator
itself still sees one flat module. With that and unary minus,
picorv32's hardware divider — a separate module it instantiates — runs
the divide and remainder tests, matching Icarus.

Adding unary minus also exposed a defect that predates it, and fixing it
comes before any new language support: an arithmetic result that wraps
around its bit width isn't reduced to that width before a right shift or
comparison reads it. With 8-bit values, Verilog gives `(3 - 5) >> 1` as
127; Ictus gives 255. It's kept as a runnable reproduction in the test
suite until it's fixed. Cranelift JIT codegen comes after that.

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
