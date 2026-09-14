//! Differential test for variable bit-select (`data[idx]` with `idx` a
//! signal, not a literal).

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn dynsel_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("dynsel_test.v");
    let testbench = fixtures.join("dynsel_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on dynsel_test's output trace"
    );
    // data = 8'b1011_0010 -- bit0=0, bit1=1, bit2=0, bit3=0, bit4=1,
    // bit5=1, bit6=0, bit7=1, sampled in that order (idx 0 through 7).
    assert_eq!(ictus_trace, vec![0, 1, 0, 0, 1, 1, 0, 1]);
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("dynsel_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    sim.set("data", 0b1011_0010);
    for idx in 0..8u64 {
        sim.set("idx", idx);
        sim.tick();
        trace.push(sim.get("bit_out"));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-dynsel");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("dynsel_test_tb.vvp");

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
