//! Differential test for the wider operator/literal/internal-signal
//! support added alongside the counter example -- same idea as
//! differential_counter.rs, exercising `& | ^ == != < <= > >= && ||`,
//! hex/binary literals, and an internal (non-port) register.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

const OUTPUTS: &[&str] = &[
    "and_result",
    "or_result",
    "xor_result",
    "eq_flag",
    "ne_flag",
    "lt_flag",
    "le_flag",
    "gt_flag",
    "ge_flag",
    "logic_flag",
    "scratch", // internal signal -- read directly via ictus_kernel, and via
               // a hierarchical reference (`uut.scratch`) in the Icarus
               // testbench.
];

#[test]
fn ops_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("ops_test.v");
    let testbench = fixtures.join("ops_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on ops_test's output trace"
    );
    assert_eq!(ictus_trace.len(), 6, "sanity check: expected 6 sampled cycles");
}

fn run_ictus(design: &Path) -> Vec<Vec<u64>> {
    let module = ictus_frontend_verilog::lower_file(design).expect("ops_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, a: u64, b: u64| {
        sim.set("resetn", resetn);
        sim.set("a", a);
        sim.set("b", b);
        sim.tick();
        trace.push(OUTPUTS.iter().map(|name| sim.get(name)).collect::<Vec<_>>());
    };

    drive(&mut sim, 0, 0, 0); // cycle 1: reset
    drive(&mut sim, 0, 0, 0); // cycle 2: reset
    drive(&mut sim, 1, 5, 5); // cycle 3: a == b
    drive(&mut sim, 1, 3, 9); // cycle 4: a < b
    drive(&mut sim, 1, 9, 3); // cycle 5: a > b
    drive(&mut sim, 1, 0xFF, 0x0F); // cycle 6: real bit patterns

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Vec<u64>> {
    let out_dir = std::env::temp_dir().join("ictus-differential-ops");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("ops_test_tb.vvp");

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
        .filter(|fields: &Vec<u64>| fields.len() == OUTPUTS.len())
        .collect()
}
