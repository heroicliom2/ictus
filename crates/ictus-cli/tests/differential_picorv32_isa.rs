//! Differential test running picorv32's own per-instruction tests -- the
//! riscv-tests rv32ui suite vendored with it, 37 programs covering every
//! base RV32I instruction -- on the vendored core, in Icarus and in Ictus.
//!
//! This is the "longer, more demanding program" step. The hand-assembled
//! program in `differential_picorv32.rs` is eight instructions; these are
//! written by the RISC-V project to exercise each instruction's corner
//! cases (sign extension, overflow, every branch direction, byte and
//! halfword accesses at every offset, writes to x0, back-to-back
//! dependencies), about 49,000 cycles in all.
//!
//! **How it runs.** The same trace replay as `differential_picorv32.rs`,
//! for the same reason: Ictus can't run a testbench (it doesn't model
//! `initial` blocks, delays or `$display`), and writing the memory model
//! twice would put two
//! hand-written models on the two sides of the comparison. Icarus runs
//! `picorv32_isa_tb.v` with the program chosen by `+program=`, recording
//! its inputs and outputs every cycle; each run is replayed into a fresh
//! Ictus simulation. picorv32 is lowered once and shared -- lowering is
//! the slow part.
//!
//! **Three checks per program**, in this order:
//!
//!   1. Icarus itself printed `<name>..OK`. Otherwise the reference run is
//!      broken and comparing against it proves nothing -- reported as such
//!      rather than as an Ictus failure.
//!   2. Every traced output matches, every cycle. The first mismatch ends
//!      that program's replay: from then on Ictus is being fed responses
//!      to requests it didn't make, so later mismatches are noise.
//!   3. The final register file matches, which is the check the ports
//!      can't make (see `differential_picorv32.rs` on a core that
//!      reproduced every bus cycle while executing nothing).
//!
//! **Configurations.** The programs run against five configurations of
//! picorv32, each applied to Ictus with `lower_file_with_parameters` and
//! to Icarus with generated `defparam`s. Two questions about them are easy
//! to conflate, and were measured separately:
//!
//!   * *Does a configuration exercise different logic?* Yes, whenever an
//!     override is applied: that configuration's logic then runs in Ictus
//!     and has to match Icarus, whether or not its bus trace happens to
//!     differ from the default's.
//!   * *Would this test notice if Ictus silently ignored an override?*
//!     Only when the override changes the bus trace. Measured one at a
//!     time on these programs: `BARREL_SHIFTER` and `TWO_STAGE_SHIFT`
//!     change 7 of 37 traces, `ENABLE_REGS_DUALPORT` 3, `TWO_CYCLE_ALU` 1,
//!     and `TWO_CYCLE_COMPARE`, the counters, `CATCH_MISALIGN`,
//!     `LATCHED_MEM_RDATA` and `COMPRESSED_ISA` none. With every override
//!     dropped on the Ictus side, `fast` fails all 37 and `small` 7 --
//!     and `compressed` passed, which is why it now runs rv32ic programs
//!     instead. That the override mechanism itself works is established
//!     directly, by ictus-frontend-verilog's own tests.
//!
//! **What isn't covered**: the four multiply tests, which need
//! `ENABLE_MUL`. The multiplier is a separate module picorv32 instantiates
//! -- instantiation works (the `divider` configuration runs the divide and
//! remainder tests through one) -- but its body uses `for` loops Ictus
//! can't lower yet. The images are built by `bench/isa/build.sh` and
//! committed, so this test needs no RISC-V toolchain. See
//! docs/decisions.md D28, D29 and D30.

use ictus_ir::Module;
use ictus_kernel::Simulation;
use std::path::{Path, PathBuf};
use std::process::Command;

const INPUTS: [&str; 3] = ["resetn", "mem_ready", "mem_rdata"];
const OUTPUTS: [&str; 8] = [
    "trap",
    "mem_valid",
    "mem_instr",
    "mem_addr",
    "mem_wdata",
    "mem_wstrb",
    "mem_la_read",
    "mem_la_write",
];

/// One recorded negedge sample: every traced field, with a value Icarus
/// printed as `x` becoming `None`.
type TraceRow = Vec<Option<u64>>;

struct IcarusRun {
    rows: Vec<TraceRow>,
    /// What the program printed through its character port.
    printed: String,
    /// `(register, value)` for every register Icarus reported a defined
    /// value for; one the program never wrote reads `x` and is left out.
    registers: Vec<(usize, u64)>,
}

/// picorv32 exactly as vendored, every parameter at its default.
#[test]
fn default_configuration_matches_icarus_verilog() {
    run_configuration("default", RV32I, &[]);
}

/// The "optimize for speed" direction: single-cycle barrel shifts, and a
/// registered ALU and comparator. `TWO_CYCLE_ALU` selects the *other*
/// branch of picorv32's `generate if (TWO_CYCLE_ALU)` -- the clocked ALU
/// that decisions.md D27 found being lowered alongside the combinational
/// one -- so this is the first run in which that branch is the live one.
#[test]
fn fast_configuration_matches_icarus_verilog() {
    run_configuration(
        "fast",
        RV32I,
        &[
            ("BARREL_SHIFTER", 1),
            ("TWO_CYCLE_ALU", 1),
            ("TWO_CYCLE_COMPARE", 1),
        ],
    );
}

/// The "optimize for area" direction: one register-file read port, a
/// one-bit-per-cycle shifter, no cycle/instruction counters, and no
/// misalignment trap.
#[test]
fn small_configuration_matches_icarus_verilog() {
    run_configuration(
        "small",
        RV32I,
        &[
            ("ENABLE_REGS_DUALPORT", 0),
            ("TWO_STAGE_SHIFT", 0),
            ("ENABLE_COUNTERS", 0),
            ("ENABLE_COUNTERS64", 0),
            ("CATCH_MISALIGN", 0),
        ],
    );
}

/// The compressed-instruction (RVC) front end, and reading memory data
/// directly rather than latching it.
///
/// This runs the *rv32ic* images, not the rv32i ones, and the reason was
/// measured rather than assumed: on plain rv32i programs a picorv32 with
/// `COMPRESSED_ISA` produces a bus trace identical to the default's, so
/// the compressed-instruction decoder never runs, and Ictus could have
/// ignored the override entirely and still passed. Assembled for rv32ic,
/// about half the instructions are 16-bit and 32-bit ones land on 2-byte
/// boundaries; 35 of these 37 programs fail on a core *without*
/// `COMPRESSED_ISA`.
#[test]
fn compressed_configuration_matches_icarus_verilog() {
    run_configuration(
        "compressed",
        RV32IC,
        &[("COMPRESSED_ISA", 1), ("LATCHED_MEM_RDATA", 1)],
    );
}

/// picorv32's hardware divider, which is a separate module
/// (`picorv32_pcpi_div`) that picorv32 instantiates when `ENABLE_DIV` is
/// set -- so this is the first run of the design in which Ictus has
/// flattened an instance, and the instance is doing real work: every
/// `div`, `divu`, `rem` and `remu` in these programs is computed by it,
/// over the co-processor interface, across many cycles. See
/// docs/decisions.md D30.
///
/// Only the four divide/remainder programs run; the multiply ones need
/// `ENABLE_MUL`, whose multiplier uses `for` loops Ictus can't lower yet.
#[test]
fn divider_configuration_matches_icarus_verilog() {
    run_configuration("divider", RV32IM, &[("ENABLE_DIV", 1)]);
}

/// The image sets `bench/isa/build.sh` produces.
const RV32I: &str = "picorv32_isa";
const RV32IC: &str = "picorv32_isa_c";
const RV32IM: &str = "picorv32_isa_m";

/// The programs in the rv32im set that the divider alone can run.
const DIVIDER_PROGRAMS: [&str; 4] = ["div", "divu", "rem", "remu"];

/// Runs a set of programs against picorv32 with `overrides` applied -- to
/// Ictus through `lower_file_with_parameters`, and to Icarus through a
/// generated module of `defparam`s, built from the same list so the two
/// can't describe different configurations.
fn run_configuration(label: &str, image_set: &str, overrides: &[(&str, u64)]) {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let design = root.join("bench/designs/picorv32/picorv32.v");
    let testbench = root.join("crates/ictus-cli/tests/fixtures/picorv32_isa_tb.v");
    let images_dir = root.join("crates/ictus-cli/tests/fixtures").join(image_set);

    let mut images: Vec<PathBuf> = std::fs::read_dir(&images_dir)
        .expect("the ISA test images should be committed; see bench/isa/build.sh")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hex"))
        .collect();
    images.sort();
    match image_set {
        // The divide/remainder programs are only half the M set; the
        // multiply half needs a multiplier this configuration doesn't have.
        RV32IM => {
            images.retain(|path| {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                DIVIDER_PROGRAMS.contains(&name)
            });
            assert_eq!(
                images.len(),
                DIVIDER_PROGRAMS.len(),
                "expected the divide/remainder images; rebuild them with bench/isa/build.sh"
            );
        }
        _ => assert_eq!(
            images.len(),
            37,
            "expected all 37 base-ISA test images; rebuild them with bench/isa/build.sh"
        ),
    }

    let compiled = compile_testbench(&design, &testbench, label, overrides);
    let module = ictus_frontend_verilog::lower_file_with_parameters(&design, overrides)
        .unwrap_or_else(|e| panic!("picorv32 should lower in the {label} configuration: {e}"));

    let mut compared = 0usize;
    let mut total = 0usize;
    let mut failures = Vec::new();
    for image in &images {
        let name = image
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("image names are plain ASCII");
        let run = run_icarus(&compiled, image);

        let expected_text = format!("{name}..OK\n");
        if run.printed != expected_text {
            failures.push(format!(
                "{name}: the *reference* run didn't pass (Icarus printed {:?}), so there is \
                 nothing trustworthy to compare Ictus against",
                run.printed
            ));
            continue;
        }

        total += run.rows.len().saturating_sub(1) * OUTPUTS.len();
        match replay(&module, &run) {
            Ok(points) => compared += points,
            Err(reason) => failures.push(format!("{name}: {reason}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} picorv32 ISA tests disagree between Ictus and Icarus Verilog in the {label} \
         configuration:\n  {}",
        failures.len(),
        images.len(),
        failures.join("\n  ")
    );
    // A field Icarus reports as `x` is skipped rather than compared. This
    // makes sure skipping can't quietly grow until little is being
    // checked. It is a *proportion* on purpose -- an absolute count would
    // depend on how many programs a configuration runs, which is how the
    // first version of it (a flat 300,000) failed the four-program divider
    // run while that run was agreeing with Icarus on every point. Measured
    // at 99.1-99.5% across the configurations; almost all of the rest is
    // `mem_wdata` before a program's first store.
    assert!(
        compared * 100 >= total * 98,
        "only {compared} of {total} output points were comparable in the {label} \n         configuration -- too many were skipped as unknown"
    );
}

/// Replays one recorded run into a fresh simulation. Returns how many
/// output points were compared, or where the two first disagreed.
fn replay(module: &Module, run: &IcarusRun) -> Result<usize, String> {
    let mut sim = Simulation::new(module);
    let mut compared = 0;

    for n in 1..run.rows.len() {
        // A recorded row holds the outputs after edge N and the inputs for
        // edge N+1 at the same instant. So: drive the previous row's
        // inputs and tick (the registers sample what was on the wire
        // before the edge), then drive this row's before reading (a
        // combinational output reacts to them with no edge in between).
        // `set_all`, because the testbench changes them in one instant.
        drive(&mut sim, &run.rows[n - 1]);
        sim.tick();
        drive(&mut sim, &run.rows[n]);

        for (i, name) in OUTPUTS.iter().enumerate() {
            let Some(expected) = run.rows[n][INPUTS.len() + i] else {
                continue;
            };
            compared += 1;
            let got = sim.get(name);
            if got != expected {
                return Err(format!(
                    "cycle {n}: {name} is {got:#x} in Ictus but {expected:#x} in Icarus"
                ));
            }
        }
    }

    for &(register, expected) in &run.registers {
        let got = sim.get_array("cpuregs", register);
        if got != expected {
            return Err(format!(
                "after the run, x{register} is {got:#x} in Ictus but {expected:#x} in Icarus"
            ));
        }
    }

    Ok(compared)
}

fn drive(sim: &mut Simulation, row: &TraceRow) {
    let values: Vec<(&str, u64)> = INPUTS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            (
                *name,
                row[i].expect("the testbench always drives defined inputs"),
            )
        })
        .collect();
    sim.set_all(&values);
}

/// Compiles the testbench for one configuration.
///
/// Icarus's own override flag, `-P`, only reaches *root* modules, and
/// picorv32 is instantiated inside the testbench. So the overrides are
/// written out as hierarchical `defparam`s in a small generated module
/// compiled alongside it. Writing them from the same list Ictus receives
/// is the point: hardcoding picorv32's parameters into the testbench
/// instead would duplicate its defaults, and a drift between the copies
/// would make the "default" run quietly not be one.
///
/// Each configuration builds in its own directory, since cargo runs these
/// tests in parallel.
fn compile_testbench(
    design: &Path,
    testbench: &Path,
    label: &str,
    overrides: &[(&str, u64)],
) -> PathBuf {
    let out_dir = std::env::temp_dir()
        .join("ictus-differential-picorv32-isa")
        .join(label);
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("picorv32_isa_tb.vvp");

    let mut defparams = String::from("module ictus_overrides;\n");
    for (name, value) in overrides {
        defparams.push_str(&format!(
            "    defparam picorv32_isa_tb.uut.{name} = {value};\n"
        ));
    }
    defparams.push_str("endmodule\n");
    let overrides_file = out_dir.join("overrides.v");
    std::fs::write(&overrides_file, defparams).expect("failed to write the overrides module");

    let compile = Command::new("iverilog")
        .arg("-o")
        .arg(&compiled)
        .arg(testbench)
        .arg(&overrides_file)
        .arg(design)
        .output()
        .expect("failed to invoke iverilog");
    assert!(
        compile.status.success(),
        "iverilog compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    compiled
}

fn run_icarus(compiled: &Path, image: &Path) -> IcarusRun {
    let mut program_arg = std::ffi::OsString::from("+program=");
    program_arg.push(image);
    let run = Command::new("vvp")
        .arg(compiled)
        .arg(program_arg)
        .output()
        .expect("failed to invoke vvp");
    assert!(
        run.status.success(),
        "vvp run failed for {}:\n{}",
        image.display(),
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8(run.stdout).expect("vvp output should be UTF-8");

    let mut rows = Vec::new();
    let mut printed = String::new();
    let mut registers = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("T") => {
                let row: TraceRow = fields.map(|f| f.parse::<u64>().ok()).collect();
                if row.len() == INPUTS.len() + OUTPUTS.len() {
                    rows.push(row);
                }
            }
            Some("C") => {
                if let Some(code) = fields.next().and_then(|f| f.parse::<u8>().ok()) {
                    printed.push(char::from(code));
                }
            }
            Some("R") => {
                let register = fields.next().and_then(|f| f.parse::<usize>().ok());
                let value = fields.next().and_then(|f| f.parse::<u64>().ok());
                if let (Some(register), Some(value)) = (register, value) {
                    registers.push((register, value));
                }
            }
            _ => {}
        }
    }

    IcarusRun {
        rows,
        printed,
        registers,
    }
}
