//! Differential test for `else if` chains.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn elseif_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("elseif_test.v");
    let testbench = fixtures.join("elseif_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on elseif_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0x00, 0x11, 0x22, 0x33, 0xFF],
        "expected reset, then each else-if rung in turn, then the final else"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("elseif_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, level: u64| {
        sim.set("resetn", resetn);
        sim.set("level", level);
        sim.tick();
        trace.push(sim.get("result"));
    };

    drive(&mut sim, 0, 0); // cycle 1: reset
    drive(&mut sim, 1, 0); // cycle 2: first else-if
    drive(&mut sim, 1, 1); // cycle 3: second else-if
    drive(&mut sim, 1, 2); // cycle 4: third else-if
    drive(&mut sim, 1, 3); // cycle 5: final else

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-elseif");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("elseif_test_tb.vvp");

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
