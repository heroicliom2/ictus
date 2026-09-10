//! Phase 1's actual acceptance bar (docs/roadmap.md): run a design through
//! both Ictus and a reference simulator (Icarus Verilog) and check they
//! agree, cycle for cycle -- not just that Ictus runs without crashing.

use std::path::Path;
use std::process::Command;

#[test]
fn counter_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("counter.v");
    let testbench = fixtures.join("counter_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on the counter's value trace"
    );
    assert_eq!(ictus_trace.len(), 12, "sanity check: expected 12 sampled cycles");
}

/// Drives the same reset/count sequence as `counter_tb.v`: two cycles held
/// in reset, then ten cycles counting up.
fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("counter.v should lower");
    let mut sim = ictus_kernel::Simulation::new(&module);
    let mut trace = Vec::new();

    sim.set("resetn", 0);
    for _ in 0..2 {
        sim.tick();
        trace.push(sim.get("count"));
    }

    sim.set("resetn", 1);
    for _ in 0..10 {
        sim.tick();
        trace.push(sim.get("count"));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-counter");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("counter_tb.vvp");

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

    String::from_utf8(run.stdout)
        .expect("vvp output should be UTF-8")
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .collect()
}
