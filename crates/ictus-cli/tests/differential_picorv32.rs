//! Differential test against the *whole vendored picorv32 design* -- not
//! a fixture written to exercise one feature, but a real 32-bit RISC-V
//! CPU core running a real program.
//!
//! **Why this is shaped as a trace replay.** Ictus can't run a testbench
//! -- that needs `initial` blocks, delays, event waits and `$display`,
//! which it doesn't model -- so it can't run one that wraps the core in a
//! memory. (When this was written it couldn't instantiate modules either;
//! it now can, but that was never the whole obstacle.)
//! Reimplementing that memory in Rust would mean two hand-written models
//! that are *supposed* to match cycle for cycle, and any disagreement
//! between them would look exactly like a simulator bug. Instead Icarus
//! runs the testbench and records both what it drove into picorv32 and
//! what picorv32 produced; this test replays the recorded inputs into
//! Ictus and compares the outputs. The stimulus is then identical by
//! construction, and a difference can only mean the two simulators
//! interpreted the same design differently.
//!
//! **Timing.** The testbench samples at each negedge, where nothing is
//! moving, so a recorded row holds picorv32's outputs after edge N *and*
//! the inputs it will see at edge N+1 simultaneously. Replaying a row
//! therefore takes two steps: drive the previous row's inputs and tick
//! (the registers sample what was on the wire before the edge), then
//! drive this row's inputs before reading (a combinational output like
//! `mem_xfer` reacts to an input with no edge in between). Getting this
//! backwards is not a subtle inaccuracy -- it moves every combinational
//! output by a full cycle.
//!
//! **Unknown values.** A field Icarus reports as `x` is skipped rather
//! than compared: Ictus is a 2-state kernel and answers 0, which is a
//! different-but-valid answer to a question the LRM leaves open for such
//! a tool (decisions.md D6/D19), not a bug. The test asserts a floor on
//! how many points were actually compared, so this can't quietly erode
//! into a test that skips everything and passes.
//!
//! **Why the register file is checked too, and not just the ports.** For
//! straight-line code the fetch addresses don't depend on any register
//! value, so a core whose datapath is completely dead still reproduces
//! the bus trace exactly. That is not hypothetical: it was the state of
//! this simulator for one increment, with every port matching while
//! picorv32 wrote no registers at all, because `always @*` blocks -- where
//! picorv32 computes its register writes -- were being dropped. Agreement
//! on a design's ports is not evidence that the design ran.
//!
//! This test is what found the defects in docs/decisions.md D25 and D26,
//! every one of which made the core lower cleanly, run without error, and
//! execute the wrong program.

use ictus_kernel::Simulation;
use std::path::Path;
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

#[test]
fn picorv32_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let design = root.join("bench/designs/picorv32/picorv32.v");
    let testbench = root.join("crates/ictus-cli/tests/fixtures/picorv32_trace_tb.v");
    let (rows, registers) = run_icarus(&design, &testbench);

    assert!(
        rows.len() > 70,
        "expected a full trace from the testbench, got {} rows",
        rows.len()
    );

    let module = ictus_frontend_verilog::lower_file(&design)
        .expect("the vendored picorv32.v should lower cleanly");
    let mut sim = Simulation::new(&module);

    let mut compared = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();
    for n in 1..rows.len() {
        for (i, name) in INPUTS.iter().enumerate() {
            sim.set(name, rows[n - 1][i].expect("the testbench always drives defined inputs"));
        }
        sim.tick();
        for (i, name) in INPUTS.iter().enumerate() {
            sim.set(name, rows[n][i].expect("the testbench always drives defined inputs"));
        }
        for (i, name) in OUTPUTS.iter().enumerate() {
            match rows[n][INPUTS.len() + i] {
                None => skipped += 1,
                Some(expected) => {
                    compared += 1;
                    let got = sim.get(name);
                    if got != expected && failures.len() < 10 {
                        failures.push(format!("cycle {n}: {name} icarus={expected} ictus={got}"));
                    }
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Ictus and Icarus Verilog disagree while running picorv32:\n  {}",
        failures.join("\n  ")
    );
    assert!(
        compared > 500,
        "only {compared} output points were comparable ({skipped} skipped as unknown); \
         the test would pass without checking anything meaningful"
    );

    // Final architectural state -- the program's actual result, read out
    // of picorv32's own register file on both sides. This is the check
    // the port comparison above cannot make: for straight-line code the
    // fetch addresses don't depend on any register value, so a core with
    // a completely dead datapath still reproduces the bus trace exactly.
    // It did, for a while -- see the note in this file's header.
    //
    // It also reads an unpacked array at design scale, since picorv32's
    // register file is one.
    assert_eq!(
        registers,
        vec![(1, 5), (2, 7), (3, 12), (4, 12), (5, 7)],
        "expected the testbench program to have executed under Icarus"
    );
    for (index, expected) in &registers {
        assert_eq!(
            sim.get_array("cpuregs", *index as usize),
            *expected,
            "register x{index} differs from Icarus after the program ran"
        );
    }
}

/// One recorded negedge sample: every traced field, with a value Icarus
/// printed as `x` becoming `None`.
type TraceRow = Vec<Option<u64>>;

/// Runs the testbench and returns the per-cycle trace rows plus the
/// `(register, value)` pairs from the final register-file dump.
fn run_icarus(design: &Path, testbench: &Path) -> (Vec<TraceRow>, Vec<(u64, u64)>) {
    let out_dir = std::env::temp_dir().join("ictus-differential-picorv32");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("picorv32_trace_tb.vvp");

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

    let run = Command::new("vvp")
        .arg(&compiled)
        .output()
        .expect("failed to invoke vvp");
    assert!(
        run.status.success(),
        "vvp run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let text = String::from_utf8(run.stdout).expect("vvp output should be UTF-8");

    let mut rows = Vec::new();
    let mut registers = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("R ") {
            let mut fields = rest.split_whitespace();
            let index = fields.next().and_then(|f| f.parse::<u64>().ok());
            let value = fields.next().and_then(|f| f.parse::<u64>().ok());
            // A register the program never wrote reads as `x`; only the
            // ones it actually set are comparable.
            if let (Some(index), Some(value)) = (index, value) {
                registers.push((index, value));
            }
        } else if !line.trim().is_empty() {
            let row: TraceRow = line
                .split_whitespace()
                .map(|f| f.parse::<u64>().ok())
                .collect();
            if row.len() == INPUTS.len() + OUTPUTS.len() {
                rows.push(row);
            }
        }
    }
    (rows, registers)
}
