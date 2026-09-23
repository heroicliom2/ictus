//! Differential test for signed ordering comparisons
//! (`$signed(a) < $signed(b)` and friends) -- picorv32's own
//! `alu_lts <= $signed(reg_op1) < $signed(reg_op2);`. The first two
//! cases have exactly one operand's sign bit set, so the signed and
//! unsigned readings genuinely disagree: the `unsigned_lt` control
//! column must come out *opposite* to `lt` there, which is what proves
//! the comparison is really signed rather than passing by accident.
//! See ictus_ir::Expr::SignedLt.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64, u64);

#[test]
fn signed_compare_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("signed_compare_test.v");
    let testbench = fixtures.join("signed_compare_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on signed_compare_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (lt, gt, le, ge, unsigned_lt)
            // -128 < 1 signed; 128 < 1 is false unsigned -- they disagree.
            (1, 0, 1, 0, 0),
            // 1 < -128 is false signed; 1 < 128 is true unsigned.
            (0, 1, 0, 1, 1),
            // 5 vs 5: equal.
            (0, 0, 1, 1, 0),
            // -1 vs -2: greater, signed.
            (0, 1, 0, 1, 0),
            // -2 vs -1: less, signed.
            (1, 0, 1, 0, 1),
        ],
        "expected signed <, >, <=, >= plus the unsigned < control"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("signed_compare_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, a: u64, b: u64| {
        sim.set("a", a);
        sim.set("b", b);
        sim.tick();
        trace.push((
            sim.get("lt"),
            sim.get("gt"),
            sim.get("le"),
            sim.get("ge"),
            sim.get("unsigned_lt"),
        ));
    };

    drive(&mut sim, 0x80, 0x01);
    drive(&mut sim, 0x01, 0x80);
    drive(&mut sim, 0x05, 0x05);
    drive(&mut sim, 0xFF, 0xFE);
    drive(&mut sim, 0xFE, 0xFF);

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-signed-compare");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("signed_compare_test_tb.vvp");

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
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let a = fields.next()?.parse::<u64>().ok()?;
            let b = fields.next()?.parse::<u64>().ok()?;
            let c = fields.next()?.parse::<u64>().ok()?;
            let d = fields.next()?.parse::<u64>().ok()?;
            let e = fields.next()?.parse::<u64>().ok()?;
            Some((a, b, c, d, e))
        })
        .collect()
}
