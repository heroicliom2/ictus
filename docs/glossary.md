# Glossary

Two kinds of jargon show up in this repo's docs: HDL/verification terms
(which this project's target audience — electrical engineers — will mostly
already know) and software/compiler-engineering terms (which an EE
background doesn't automatically include, even at the master's level, since
they belong to a different field: programming-language implementation).
This file explains both in full, in plain language, with no assumed
compiler-theory background. Every doc in this repo should give a short
inline gloss the first time it uses one of the software-engineering terms
below; this file is where the complete explanation lives.

## HDL / EDA terms

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
order (see "Topological sort" below), skipping fine-grained event-queue
overhead. Much faster for synchronous RTL; trades away exact sub-cycle
delta-cycle fidelity. This is what Verilator does and what Ictus's primary
kernel does (see decisions.md D2).

**2-state / 4-state logic** — 2-state: signals are only 0 or 1 (fast,
bit-packed — see "Bit-packing" below). 4-state: signals can also be X
(unknown) or Z (high-impedance), which is closer to real hardware
semantics (e.g. uninitialized registers) but far more expensive to
simulate. See decisions.md D6.

**MTask** — Verilator's term for a statically-scheduled (see "Static vs.
dynamic scheduling" below) unit of parallel work produced by partitioning
the design's dataflow graph at compile time, rather than dynamically
scheduling individual events across threads.

**SVA** — SystemVerilog Assertions; a temporal-logic sublanguage for
expressing "this must always/eventually hold" properties, checked during
simulation (or by formal tools). Needs scheduling-region support beyond the
simplified active/inactive/postponed model. Deferred, not rejected — see
decisions.md D10/D11.

**UVM** — Universal Verification Methodology; a large SystemVerilog
class-library/methodology standard for building testbenches
(randomization, coverage, transaction-level modeling). Deferred, not
rejected — see decisions.md D10/D11.

**DPI-C / VPI / PLI** — C foreign-function interfaces (see "FFI" below)
that let SystemVerilog code call out to (or be called from) compiled C/C++.
This is how most real-world testbenches and legacy verification IP
interoperate with a simulator. Not yet on the roadmap; would matter for
real-world adoption beyond the benchmark-suite stage.

**Gate-level simulation / SDF** — simulating a design after synthesis, at
the level of actual logic gates, with real timing delays back-annotated
from a Standard Delay Format (SDF) file. Different (and much slower/more
detailed) than RTL simulation. Deferred, not rejected — see decisions.md
D10/D11.

**FST** — GTKWave's compressed waveform trace file format; what modern
waveform viewers (GTKWave, Surfer) prefer over plain VCD for size and load
speed.

**VCD** — Value Change Dump; the original, uncompressed, text-based
Verilog waveform trace format.

**Netlist** — a design description as an explicit graph of gates/cells and
the wires connecting them, as opposed to behavioral RTL code.

## Software & compiler engineering terms

**Compiler** — a program that translates code written in one language into
another form, usually one closer to what a machine can execute directly.
A compiler doesn't have to run the code itself, just translate it — a
separate step (or the same step, depending on the approach — see AOT vs.
JIT below) actually executes the result.

**AST (Abstract Syntax Tree)** — a tree-shaped data structure representing
the grammatical structure of a piece of source code: e.g. "this is a
module, containing these statements; this statement is an assignment,
with this expression on the left and this expression on the right." It's
the standard first output of parsing any programming/HDL language — not
something specific to this project — and is the input the rest of a
compiler or analysis tool works from. "Lexing" is the step just before
building an AST: splitting raw source text into meaningful chunks
(keywords, identifiers, numbers, punctuation) before assembling the tree
structure from them.

**LSP (Language Server Protocol)** — a standard protocol that lets a code
editor (VS Code, etc.) get live diagnostics, go-to-definition,
autocomplete, and similar features from a separate "language server"
program that actually understands the language. `vhdl_lang`/`vhdl_ls`
(docs/architecture.md, VHDL frontend) is built around this protocol.

**Intermediate representation (IR)** — a general term for *any* internal
data structure a compiler or similar tool uses to represent a program
in-between reading the original source and producing the final output. An
IR isn't meant to be written by hand; it exists purely to make the next
processing step easier than working directly on either the original source
text or the final machine code. This project has its own specific IR,
the `ictus-ir` crate (see docs/architecture.md) — that's a concrete instance
of this general concept, not a different thing.

**AOT (Ahead-Of-Time) compilation** — translating source code into a
runnable form as a distinct step that happens *before* execution starts,
producing an artifact (e.g. an `.exe` file) you can then run separately,
any number of times, without recompiling. Ordinary C/C++ development works
this way: you run a compiler (like GCC or Clang) once to produce a program,
then run that program later. Verilator uses this model for HDL simulation:
it translates your Verilog/SystemVerilog design into C++ source code, and
you must then separately invoke a C++ compiler to turn that into an actual
runnable simulator — two distinct steps, with the second one (a full C++
compile) potentially taking minutes for a large design.

**JIT (Just-In-Time) compilation** — translating code into real machine
instructions at the moment it's about to run, inside the same running
program, rather than as a separate build step that happens first. There's
no separate compiler invocation, no intermediate files written to disk, and
no second toolchain to install or manage — the tool that's simulating your
design is also the thing turning it into machine code, on the spot. This is
the approach Ictus uses (via Cranelift, below) specifically because it
collapses Verilator's two-step "generate C++, then separately compile it"
process into one fast in-process step — see decisions.md D3.

**Cranelift** — a specific, open-source code generator: a piece of software
that takes a low-level program representation and produces real, directly
executable CPU machine instructions from it, at runtime. It's written in
Rust (the same language this project is written in) and was originally
built for, and is still used by, Wasmtime (a runtime for executing
WebAssembly programs). Ictus uses it to implement JIT compilation (above):
Cranelift is the actual component doing the "translate the elaborated
design into machine code, right now, in this process" work. It's chosen
over the alternative below because it prioritizes fast compilation over
squeezing out the absolute best-possible machine code.

**LLVM** — a much larger, older, and more widely used compiler
infrastructure project, also capable of turning an intermediate
representation into machine code. Generally produces more
highly-optimized (faster-running) output than Cranelift does, but takes
noticeably longer to do the translation. It's the backend behind real
production C/C++ compilers (Clang) and behind Rust's own compiler
(`rustc`). Mentioned in this project's docs as the likely choice for an
optional, slower-to-build-but-faster-to-run path (docs/roadmap.md phase
6) for cases like CI runs where build time matters less than raw
execution speed — Cranelift stays the fast default for everyday use.

**Lowering** — translating a program from a higher-level representation
into a lower-level, simpler, or more explicit one that's closer to what
the next processing stage needs — e.g., turning a frontend's AST into this
project's shared IR (docs/architecture.md, Pipeline). "Lower" just means
"more explicit/mechanical, less abstract," not "worse."

**Codegen (code generation)** — the step where a compiler produces the
final output form — actual CPU machine instructions, in this project's
case — from whatever representation it's been working with internally.
"Fast codegen" means that step itself is quick to run.

**Backend** — in compiler terminology, the part of a toolchain responsible
for the final translation into machine code (as opposed to the "frontend,"
which parses source into an AST). Cranelift and LLVM (both explained
above) are both examples of compiler backends — different components that
can each do this final translation step, with different speed/output-quality
tradeoffs.

**Dataflow graph** — a way of representing a computation as a graph
instead of a list of statements: each node is an operation (e.g. "this AND
gate," "this always-block"), and each edge is a piece of data flowing from
one operation into another one that depends on it. Representing a design
this way — rather than as a flat sequence of statements — is what makes it
possible to figure out, ahead of time, which parts of a design don't
depend on each other (and can therefore be reordered, evaluated in
parallel, or skipped when nothing they depend on changed).

**Topological sort / topologically sorted** — an ordering of the nodes in a
dependency graph such that every node appears only after everything it
depends on. For a design's logic, this means: don't compute a signal's new
value until every signal it reads has already been computed for this
cycle — otherwise the result would be built from a stale, not-yet-updated
value. This is a standard, well-known graph algorithm, not something
specific to this project.

**Static vs. dynamic scheduling** — two different ways to decide which
thread runs which piece of work. *Static*: the work is divided up once, in
advance, before execution starts (e.g., "thread 1 always evaluates these
40 always-blocks, thread 2 always evaluates these other 40"), and that
assignment never changes while running. *Dynamic*: which thread runs which
piece of work is decided on the fly, while the program is running. Static
scheduling has less runtime overhead (no decision-making while the clock is
ticking) but only works well when the dependency structure is fixed and
known in advance — which is exactly true of a hardware design once it's
elaborated, which is why Ictus uses the static approach (decisions.md D5).

**Work-stealing** — a specific dynamic-scheduling technique (the opposite
of the static approach above): each thread keeps its own queue of small
pending tasks, and whenever a thread runs out of work, it "steals" a task
from a still-busy thread's queue instead of sitting idle. Good for
irregular, unpredictable workloads. This project considered and rejected
it (in favor of static partitioning) for the core simulation kernel,
because individual HDL simulation events are too small and numerous for
the bookkeeping overhead of stealing work at runtime to pay off — see
decisions.md D5.

**SIMD (Single Instruction, Multiple Data)** — a CPU feature that applies
the same single operation to several pieces of data at once, in one
instruction, instead of processing one piece of data per instruction.
Relevant here because simulating a large number of individual signal bits
in parallel is exactly the kind of repetitive, uniform workload SIMD
speeds up — which is why the signal representation (docs/architecture.md,
"Signal representation") is deliberately laid out in memory in a way that
lets the CPU apply SIMD instructions to it.

**Bit-packing** — storing many small values tightly together inside larger
machine words, instead of spending a whole byte (or a whole data
structure, like an enum with a tag) on each individual value. For example,
64 individual 1-bit signals can be packed into a single 64-bit integer
rather than 64 separate bytes/objects. Far more memory- and
cache-efficient, which matters enormously at simulation scale where a
design may have millions of signal-updates per second.

**FFI (Foreign Function Interface)** — a mechanism that lets code written
in one programming language call code written in a different one (most
commonly: calling into compiled C code from something else). DPI-C/VPI/PLI
(HDL section, above) are specific FFI mechanisms defined by the Verilog/SV
standards for calling C/C++ from a testbench.

**Crate / workspace (Rust-specific)** — in Rust, a *crate* is a single
buildable package: roughly, one library or one program. A *workspace* is a
group of crates that are built and version-tracked together, sharing one
dependency lock file. Ictus is organized as one workspace containing four
crates (`ictus-ir`, `ictus-frontend-verilog`, `ictus-kernel`, `ictus-cli`)
— see docs/development.md for the actual layout and build commands.

**ADR (Architecture Decision Record)** — a short, standardized document
format used across the software industry for recording one specific
technical decision: what was decided, what alternatives were considered,
and why. `docs/decisions.md` in this repo is written in this style — it's
a running log, not a single document, so past decisions stay visible even
after later ones build on or revise them.

**CI (Continuous Integration)** — automatically building the project and
running its test/benchmark suite every time code changes (rather than only
occasionally and by hand), so problems are caught immediately instead of
accumulating unnoticed. Referenced in docs/roadmap.md phase 6 as a context
where build time (AOT via LLVM, above) matters less than it does in normal
day-to-day development (JIT via Cranelift, above).

## Licensing terms

Software licenses matter here because this project deliberately avoided
reusing code from one existing project (gtkwave) over licensing concerns
— see decisions.md D7. The distinction that mattered:

**Permissive licenses (MIT, Apache-2.0, BSD-3)** — anyone can use, modify,
and redistribute the code, including inside closed-source or commercial
products, as long as they keep the original copyright/license notice
attached. This is effectively "do whatever you want with it, just don't
claim you wrote it." Ictus itself is dual-licensed MIT/Apache-2.0
(meaning: anyone using it can pick whichever of the two they prefer) — the
standard, expected choice across the Rust ecosystem, matching `sv-parser`'s
own licensing.

**Copyleft licenses (GPL, and variants)** — if you incorporate
GPL-licensed code into your own project and then distribute your project,
your own code generally has to be released under the GPL too (or a
compatible license). In practice this means you generally cannot fold
GPL-licensed code into a permissively-licensed (MIT/Apache) project without
the combined result effectively becoming GPL-licensed as a whole. This is
exactly why decisions.md D7 rules out porting code from gtkwave (which is
GPLv2-licensed): doing so would force a license change on this project
that nobody wants.

**LGPL (Lesser GPL)** — a weaker, partial form of copyleft: roughly,
*linking against* an LGPL-licensed library from your own separately-licensed
code is fine, but modifying the LGPL library itself and redistributing your
modified version is not (that part stays copyleft). Mentioned in
decisions.md D7 because one optional code path inside gtkwave is
specifically gated behind LGPL, separately from gtkwave's own GPLv2
licensing for everything else.

**EUPL (European Union Public Licence)** — the EU's own copyleft license,
similar in effect to the GPL. Mentioned because Surfer (the external
waveform-viewer project Ictus integrates *with*, as a separate program —
not code incorporated into Ictus itself) is EUPL-licensed. Integrating with
a separate EUPL-licensed program is not the same situation as incorporating
GPL code directly (see D7) — no code is being copied into this project's
own codebase.
