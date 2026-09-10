//! Differential test for `case` statement support: a plain arm, a
//! comma-joined multi-value arm, and the default arm, all checked against
//! Icarus Verilog.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn case_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("case_test.v");
    let testbench = fixtures.join("case_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on case_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0x00, 0x11, 0x22, 0x33, 0x33, 0xFF],
        "expected reset, arm 0, arm 1, the comma-joined arm (both values), then default"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("case_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, sel: u64| {
        sim.set("resetn", resetn);
        sim.set("sel", sel);
        sim.tick();
        trace.push(sim.get("result"));
    };

    drive(&mut sim, 0, 0); // cycle 1: reset
    drive(&mut sim, 1, 0); // cycle 2: arm 0
    drive(&mut sim, 1, 1); // cycle 3: arm 1
    drive(&mut sim, 1, 2); // cycle 4: comma-joined arm
    drive(&mut sim, 1, 3); // cycle 5: comma-joined arm
    drive(&mut sim, 1, 5); // cycle 6: default

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-case");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("case_test_tb.vvp");

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
