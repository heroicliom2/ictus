//! Differential test for the "default to a `x`-valued placeholder, then
//! override in every case arm" idiom picorv32 uses throughout
//! (`decoded_imm <= 1'bx;` followed by a `case` that overrides it for
//! every real encoding). This is deliberately the only shape of
//! x-literal test run differentially against Icarus Verilog: since
//! every `sel` value is covered by the case, `result` is never actually
//! left at its `x` default in *either* simulator, so this doesn't depend
//! on Ictus's own x-resolves-to-0 policy (decisions.md D19) agreeing
//! with Icarus's real 4-state 'x' -- which it fundamentally can't in
//! general, since a genuinely-undefined 4-state result has no single
//! comparable value at all. See ictus-frontend-verilog/tests/xz_literal.rs
//! for direct (structural, not differential) coverage of the resolution
//! policy itself.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn xz_default_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("xz_default_test.v");
    let testbench = fixtures.join("xz_default_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on xz_default_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0, 0x11, 0x22, 0x33, 0x44],
        "expected reset, then each case arm's override value, never the x default"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("xz_default_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, sel: u64| {
        sim.set("resetn", resetn);
        sim.set("sel", sel);
        sim.tick();
        trace.push(sim.get("result"));
    };

    drive(&mut sim, 0, 0b00); // cycle 1: reset
    drive(&mut sim, 1, 0b00); // cycle 2
    drive(&mut sim, 1, 0b01); // cycle 3
    drive(&mut sim, 1, 0b10); // cycle 4
    drive(&mut sim, 1, 0b11); // cycle 5

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-xz-default");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("xz_default_test_tb.vvp");

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
