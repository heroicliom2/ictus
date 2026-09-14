//! Differential test for bit-select/part-select as a non-blocking
//! assignment *target* (`acc[3:0] <= v;`) -- see ictus_kernel's
//! read-modify-write commit logic and
//! ictus-frontend-verilog's lower_select_target_range.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn select_target_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("select_target_test.v");
    let testbench = fixtures.join("select_target_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on select_target_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            0, 161, 178, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207, 192
        ],
        "expected reset, then the high nibble loading from `data` and the low nibble \
         self-incrementing (and wrapping at 4 bits) each cycle, as two disjoint partial \
         writes to the same register in one tick"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("select_target_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, data: u64| {
        sim.set("resetn", resetn);
        sim.set("data", data);
        sim.tick();
        trace.push(sim.get("acc"));
    };

    drive(&mut sim, 0, 0x0); // cycle 1: reset
    drive(&mut sim, 1, 0xA); // cycle 2
    drive(&mut sim, 1, 0xB); // cycle 3
    for _ in 0..14 {
        drive(&mut sim, 1, 0xC); // low nibble walks 0x3..0xF, then wraps to 0x0
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-select-target");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("select_target_test_tb.vvp");

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
