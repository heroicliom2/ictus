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
//! for the same reason: Ictus can't instantiate modules, so it can't run a
//! testbench, and writing the memory model twice would put two
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
//! **What isn't covered**: the multiply, divide and remainder tests, which
//! need `ENABLE_MUL`/`ENABLE_DIV` -- separate modules picorv32
//! instantiates, and a non-default parameter Ictus can't set yet. The
//! images are built by `bench/isa/build.sh` and committed, so this test
//! needs no RISC-V toolchain. See docs/decisions.md D28.

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

#[test]
fn picorv32_isa_tests_match_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let design = root.join("bench/designs/picorv32/picorv32.v");
    let testbench = root.join("crates/ictus-cli/tests/fixtures/picorv32_isa_tb.v");
    let images_dir = root.join("crates/ictus-cli/tests/fixtures/picorv32_isa");

    let mut images: Vec<PathBuf> = std::fs::read_dir(&images_dir)
        .expect("the ISA test images should be committed; see bench/isa/build.sh")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "hex"))
        .collect();
    images.sort();
    assert_eq!(
        images.len(),
        37,
        "expected all 37 base-ISA test images; rebuild them with bench/isa/build.sh"
    );

    let compiled = compile_testbench(&design, &testbench);
    let module = ictus_frontend_verilog::lower_file(&design)
        .expect("the vendored picorv32.v should lower cleanly");

    let mut compared = 0usize;
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

        match replay(&module, &run) {
            Ok(points) => compared += points,
            Err(reason) => failures.push(format!("{name}: {reason}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} picorv32 ISA tests disagree between Ictus and Icarus Verilog:\n  {}",
        failures.len(),
        images.len(),
        failures.join("\n  ")
    );
    // The same floor as the other picorv32 test: a field Icarus reports as
    // `x` is skipped, and this makes sure skipping can't quietly grow
    // until nothing is being checked.
    assert!(
        compared > 300_000,
        "only {compared} output points were comparable across all tests"
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

fn compile_testbench(design: &Path, testbench: &Path) -> PathBuf {
    let out_dir = std::env::temp_dir().join("ictus-differential-picorv32-isa");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("picorv32_isa_tb.vvp");

    let compile = Command::new("iverilog")
        .arg("-o")
        .arg(&compiled)
        .arg(testbench)
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
