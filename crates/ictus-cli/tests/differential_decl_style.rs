//! Differential test covering three fixes made together while lowering
//! the real picorv32 benchmark design for the first time: a port
//! inheriting its direction from the previous one in the list
//! (`input clk, resetn,`), a single declaration naming several signals
//! (`reg [7:0] a, b, c;`), and nested ternaries (including the
//! unparenthesized-comparison-condition precedence fix -- see
//! ternary_precedence.rs in ictus-frontend-verilog for that one in
//! isolation).

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn decl_style_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("decl_style_test.v");
    let testbench = fixtures.join("decl_style_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on decl_style_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![0, 0, 9],
        "expected reset, one stale cycle (result still reflects pre-edge a=b=c=0), then max(5,9,7)=9"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module = ictus_frontend_verilog::lower_file(design).expect("decl_style_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    sim.set("resetn", 0);
    sim.tick(); // unsampled settling edge -- see the testbench's matching comment

    trace.push(sim.get("result"));

    sim.set("resetn", 1);
    sim.tick();
    trace.push(sim.get("result"));
    sim.tick();
    trace.push(sim.get("result"));

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-decl-style");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("decl_style_test_tb.vvp");

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
