//! Differential test for `$signed(...)` sign-extending a narrower value
//! into a wider assignment target. See ictus_ir::Expr::Signed's doc
//! comment for the semantics and ictus_kernel::eval_expr's `Signed` arm
//! for the actual bit manipulation. Values are deliberately chosen to
//! include both sign-bit-clear (positive) and sign-bit-set (negative)
//! cases -- a zero-extension bug would only show up on the latter.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn signed_ext_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("signed_ext_test.v");
    let testbench = fixtures.join("signed_ext_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on signed_ext_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0, 1, 0x01F, 0xFE0, 0xFFF, 0xFF0],
        "expected reset, then $signed(narrow) sign-extended into the 12-bit target each cycle"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("signed_ext_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, narrow: u64| {
        sim.set("resetn", resetn);
        sim.set("narrow", narrow);
        sim.tick();
        trace.push(sim.get("wide"));
    };

    drive(&mut sim, 0, 0b000000); // cycle 1: reset
    drive(&mut sim, 1, 0b000001); // cycle 2: +1
    drive(&mut sim, 1, 0b011111); // cycle 3: +31, sign bit clear
    drive(&mut sim, 1, 0b100000); // cycle 4: -32, sign bit set
    drive(&mut sim, 1, 0b111111); // cycle 5: -1, sign bit set
    drive(&mut sim, 1, 0b110000); // cycle 6: -16, sign bit set

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-signed-ext");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("signed_ext_test_tb.vvp");

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
