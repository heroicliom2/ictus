//! Differential test for the unparenthesized-ternary-after-comparison
//! precedence fix (see ictus-frontend-verilog's E::Binary handling and
//! its own ternary_precedence.rs). Deliberately includes a case where the
//! condition is false (a <= c) and one where a == c, not just the "a
//! wins" case -- a precedence bug that only got caught on one branch of
//! the condition wouldn't be much of a check.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn ternary_precedence_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("ternary_precedence_test.v");
    let testbench = fixtures.join("ternary_precedence_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on ternary_precedence_test's output trace"
    );
    assert_eq!(ictus_trace, vec![10, 8, 5], "expected a, then c, then c (a==c case)");
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("ternary_precedence_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, a: u64, c: u64| {
        sim.set("a", a);
        sim.set("c", c);
        sim.tick();
        trace.push(sim.get("result"));
    };

    drive(&mut sim, 10, 3);
    drive(&mut sim, 2, 8);
    drive(&mut sim, 5, 5);

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-ternary-precedence");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("ternary_precedence_test_tb.vvp");

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
