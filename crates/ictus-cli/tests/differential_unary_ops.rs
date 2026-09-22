//! Differential test for unary bitwise complement (`~`) and the
//! reduction operators (`& | ^ ~& ~| ~^`) -- only logical `!` was
//! supported before this. `r_nand` mirrors picorv32's own style directly
//! (`~&mem_rdata_latched[1:0]`). See ictus_ir::Expr::BitwiseNot/
//! ReduceAnd/ReduceOr/ReduceXor and ictus_kernel::eval_expr's arms for
//! them.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64, u64, u64, u64, u64);

#[test]
fn unary_ops_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("unary_ops_test.v");
    let testbench = fixtures.join("unary_ops_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on unary_ops_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            (0, 0, 0, 0, 0, 0, 0, 0),
            (1, 15, 0, 0, 0, 1, 1, 1),
            (0, 0, 1, 1, 0, 0, 0, 1),
            (0, 10, 0, 1, 0, 1, 0, 1),
            (0, 8, 0, 1, 1, 1, 0, 0),
            (0, 5, 0, 1, 0, 1, 0, 1),
        ],
        "expected reset, then (!x, ~x, &x, |x, ^x, ~&x, ~|x, ~^x) for each x value"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module = ictus_frontend_verilog::lower_file(design).expect("unary_ops_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, x: u64| {
        sim.set("resetn", resetn);
        sim.set("x", x);
        sim.tick();
        trace.push((
            sim.get("r_not"),
            sim.get("r_bitnot"),
            sim.get("r_and"),
            sim.get("r_or"),
            sim.get("r_xor"),
            sim.get("r_nand"),
            sim.get("r_nor"),
            sim.get("r_xnor"),
        ));
    };

    drive(&mut sim, 0, 0x0); // cycle 1: reset
    drive(&mut sim, 1, 0x0); // cycle 2
    drive(&mut sim, 1, 0xF); // cycle 3
    drive(&mut sim, 1, 0x5); // cycle 4
    drive(&mut sim, 1, 0x7); // cycle 5
    drive(&mut sim, 1, 0xA); // cycle 6

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-unary-ops");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("unary_ops_test_tb.vvp");

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
            let f = fields.next()?.parse::<u64>().ok()?;
            let g = fields.next()?.parse::<u64>().ok()?;
            let h = fields.next()?.parse::<u64>().ok()?;
            Some((a, b, c, d, e, f, g, h))
        })
        .collect()
}
