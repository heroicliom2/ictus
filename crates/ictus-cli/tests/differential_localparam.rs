//! Differential test for `localparam`, sharing the same resolution pass
//! and name table as `parameter` so a `localparam` can reference an
//! earlier `parameter` -- mirrors picorv32's own `regindex_bits`/
//! `WITH_PCPI` directly (cross-parameter references, the ternary
//! operator, multiplication, and logical OR, all in constant-expression
//! contexts), plus a packed-range bound and a bit-select target index
//! that both reference a localparam (picorv32's `decoded_rd`/
//! `decoded_rs1[regindex_bits-1] <= 1;` style). See
//! ictus-frontend-verilog's lower_constant_expr/try_const_fold.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn localparam_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("localparam_test.v");
    let testbench = fixtures.join("localparam_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on localparam_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![(0, 0, 0), (7, 1, 64), (7, 1, 64)],
        "expected reset, then index_bits=7, with_feature=1, and bit 6 of wide_reg set"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("localparam_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64| {
        sim.set("resetn", resetn);
        sim.tick();
        trace.push((
            sim.get("index_bits_out"),
            sim.get("with_feature_out"),
            sim.get("wide_reg_out"),
        ));
    };

    drive(&mut sim, 0); // cycle 1: reset
    drive(&mut sim, 1); // cycle 2
    drive(&mut sim, 1); // cycle 3

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-localparam");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("localparam_test_tb.vvp");

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
            Some((a, b, c))
        })
        .collect()
}
