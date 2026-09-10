# Glossary

HDL/EDA terms used throughout this repo's docs, for readers (human or agent)
without prior simulator-domain background.

**RTL** — Register-Transfer Level; the abstraction level (clocked registers
+ combinational logic between them) that synthesizable hardware description
is normally written at.

**Elaboration** — resolving a design's static structure before simulation
can begin: generic/parameter values, generate-block expansion, module
instance hierarchy, port connections. Distinct from parsing (syntax) and
from simulation (runtime behavior).

**Event-driven simulation** — the classic simulation model: a queue of
scheduled events (a signal changes at a given simulation time), processed in
time order, where evaluating one event can schedule more. Precise but has
per-event overhead.

**Delta cycle** — a zero-simulation-time-advancing re-evaluation step within
event-driven simulation, needed because one signal change can trigger
others "in the same instant"; IEEE 1800 defines multiple ordered regions
within this (see below) specifically so results are deterministic.

**Scheduling regions (IEEE 1800)** — the full ordered list a
standards-compliant SV event-driven kernel steps through per time step:
preponed, active, inactive, NBA (non-blocking assign), observed, reactive,
re-inactive, re-NBA, postponed. The extra regions beyond
active/inactive/postponed exist for SVA assertions and program blocks.

**Cycle-based simulation** — an alternative to event-driven: evaluate all
combinational/clocked logic once per relevant clock edge, in topological
order, skipping fine-grained event-queue overhead. Much faster for
synchronous RTL; trades away exact sub-cycle delta-cycle fidelity. This is
what Verilator does and what Ictus's primary kernel does (see
decisions.md D2).

**2-state / 4-state logic** — 2-state: signals are only 0 or 1 (fast,
bit-packed). 4-state: signals can also be X (unknown) or Z (high-impedance),
which is closer to real hardware semantics (e.g. uninitialized registers)
but far more expensive to simulate. See decisions.md D6.

**MTask** — Verilator's term for a statically-scheduled unit of parallel
work produced by partitioning the design's dataflow graph at compile time,
rather than dynamically scheduling individual events across threads.

**SVA** — SystemVerilog Assertions; a temporal-logic sublanguage for
expressing "this must always/eventually hold" properties, checked during
simulation (or by formal tools). Needs scheduling-region support beyond the
simplified active/inactive/postponed model. Not currently in scope
(decisions.md D10).

**UVM** — Universal Verification Methodology; a large SystemVerilog
class-library/methodology standard for building testbenches
(randomization, coverage, transaction-level modeling). Not currently in
scope (decisions.md D10).

**DPI-C / VPI / PLI** — C foreign-function interfaces that let
SystemVerilog code call out to (or be called from) compiled C/C++. This is
how most real-world testbenches and legacy verification IP interoperate
with a simulator. Not yet on the roadmap; would matter for real-world
adoption beyond the benchmark-suite stage.

**Gate-level simulation / SDF** — simulating a design after synthesis, at
the level of actual logic gates, with real timing delays back-annotated
from a Standard Delay Format (SDF) file. Different (and much slower/more
detailed) than RTL simulation. Not currently in scope (decisions.md D10).

**FST** — GTKWave's compressed waveform trace file format; what modern
waveform viewers (GTKWave, Surfer) prefer over plain VCD for size and load
speed.

**VCD** — Value Change Dump; the original, uncompressed, text-based
Verilog waveform trace format.

**Netlist** — a design description as an explicit graph of gates/cells and
the wires connecting them, as opposed to behavioral RTL code.

**JIT vs. AOT** — Just-In-Time compilation happens at load/run time (what
Cranelift does here, decisions.md D3); Ahead-Of-Time compilation happens as
a separate build step before running (what Verilator does, generating
C++ and invoking GCC/Clang).
