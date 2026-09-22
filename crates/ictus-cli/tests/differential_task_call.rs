//! Differential test for a call to a provably-empty task
//! (`empty_statement;`, mirroring picorv32's own no-op-placeholder style
//! for a compiled-out `` `assert(...) ``) lowering as a true no-op --
//! the surrounding counter logic must behave identically whether or not
//! the (ignored) task call is present. See
//! ictus-frontend-verilog's lower_task_call_statement.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn task_call_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("task_call_test.v");
    let testbench = fixtures.join("task_call_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on task_call_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0, 1, 2, 3, 4],
        "expected reset, then count incrementing each cycle, unaffected by the task call"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("task_call_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    sim.set("resetn", 0);
    sim.tick();
    trace.push(sim.get("count"));

    sim.set("resetn", 1);
    for _ in 0..4 {
        sim.tick();
        trace.push(sim.get("count"));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-task-call");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("task_call_test_tb.vvp");

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
