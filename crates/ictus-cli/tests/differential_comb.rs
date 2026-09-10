//! Differential test for continuous `assign` / combinational-logic
//! settling (ictus_kernel::Simulation's two-settle-points-per-tick
//! design): a pure input-derived assign (`sum_comb`), a clocked register
//! that reads it (`sum_reg`), and a second assign that reads *that*
//! register (`sum_high`) -- exercising both the pre-edge and post-edge
//! settle points against a real event-driven reference simulator.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn comb_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("comb_test.v");
    let testbench = fixtures.join("comb_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on comb_test's output trace"
    );
    assert_eq!(ictus_trace.len(), 6, "sanity check: expected 6 sampled cycles");

    // Pin down the specific values the doc comments claim, so a future
    // change to the settle order fails loudly here even if it somehow
    // still matched Icarus (it shouldn't, but this is cheap insurance).
    assert_eq!(ictus_trace[2], vec![150, 150, 1], "cycle 3: sum_comb settles pre-edge, sum_high post-edge");
    assert_eq!(ictus_trace[3], vec![30, 30, 0], "cycle 4: sum_high false below 128");
    assert_eq!(ictus_trace[4], vec![44, 44, 0], "cycle 5: 8-bit wraparound (300 -> 44)");
    assert_eq!(ictus_trace[5], vec![128, 128, 1], "cycle 6: sum_high boundary at exactly 128");
}

fn run_ictus(design: &Path) -> Vec<Vec<u64>> {
    let module = ictus_frontend_verilog::lower_file(design).expect("comb_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, a: u64, b: u64| {
        sim.set("resetn", resetn);
        sim.set("a", a);
        sim.set("b", b);
        sim.tick();
        trace.push(vec![sim.get("sum_comb"), sim.get("sum_reg"), sim.get("sum_high")]);
    };

    drive(&mut sim, 0, 0, 0); // cycle 1: reset
    drive(&mut sim, 0, 0, 0); // cycle 2: reset
    drive(&mut sim, 1, 100, 50); // cycle 3
    drive(&mut sim, 1, 10, 20); // cycle 4
    drive(&mut sim, 1, 200, 100); // cycle 5
    drive(&mut sim, 1, 128, 0); // cycle 6

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Vec<u64>> {
    let out_dir = std::env::temp_dir().join("ictus-differential-comb");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("comb_test_tb.vvp");

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
        .map(|line| {
            line.split_whitespace()
                .map(|tok| tok.parse::<u64>().expect("expected a decimal value per field"))
                .collect::<Vec<_>>()
        })
        .filter(|fields: &Vec<u64>| fields.len() == 3)
        .collect()
}
