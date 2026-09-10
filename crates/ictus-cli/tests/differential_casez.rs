//! Differential test for casez wildcard-bit matching -- the payoff for
//! bit-select and case support landing first: real decode logic
//! (`casez (instr[6:0]) 7'b0000???: ...`) needs exactly this.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn casez_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("casez_test.v");
    let testbench = fixtures.join("casez_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on casez_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0xC0, 0xFF, 0xB0, 0xA0, 0xA0],
        "expected exact match, default, then the two wildcard arms (including the sel=15 boundary)"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("casez_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, sel: u64| {
        sim.set("sel", sel);
        sim.tick();
        trace.push(sim.get("result"));
    };

    drive(&mut sim, 0); // cycle 1: exact match
    drive(&mut sim, 2); // cycle 2: default
    drive(&mut sim, 5); // cycle 3: 01??
    drive(&mut sim, 9); // cycle 4: 1???
    drive(&mut sim, 15); // cycle 5: 1??? boundary

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-casez");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("casez_test_tb.vvp");

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
