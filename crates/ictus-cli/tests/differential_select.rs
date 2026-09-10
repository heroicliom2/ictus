//! Differential test for bit-select/part-select support.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn select_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("select_test.v");
    let testbench = fixtures.join("select_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on select_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            vec![0, 0, 0],
            vec![0xCD, 0xAB, 1],
            vec![0x34, 0x12, 0],
            vec![0xFF, 0xFF, 1],
        ],
        "expected reset, then low/high byte splits and msb bit for each data value"
    );
}

fn run_ictus(design: &Path) -> Vec<Vec<u64>> {
    let module = ictus_frontend_verilog::lower_file(design).expect("select_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, data: u64| {
        sim.set("resetn", resetn);
        sim.set("data", data);
        sim.tick();
        trace.push(vec![sim.get("low_byte"), sim.get("high_byte"), sim.get("msb_bit")]);
    };

    drive(&mut sim, 0, 0x0000); // cycle 1: reset
    drive(&mut sim, 1, 0xABCD); // cycle 2
    drive(&mut sim, 1, 0x1234); // cycle 3
    drive(&mut sim, 1, 0xFFFF); // cycle 4

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Vec<u64>> {
    let out_dir = std::env::temp_dir().join("ictus-differential-select");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("select_test_tb.vvp");

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
