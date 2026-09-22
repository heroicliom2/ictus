//! Differential test for comparison operators used as concatenation
//! operands (`{a==b, a<b, a>b, a!=b}`) -- each is always exactly 1 bit,
//! so this is a valid concatenation unlike `{a+b, ...}`. See
//! ictus-frontend-verilog's expr_width.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn concat_compare_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("concat_compare_test.v");
    let testbench = fixtures.join("concat_compare_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on concat_compare_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![8, 5, 3],
        "expected {{a==b, a<b, a>b, a!=b}} packed for each (a, b) pair"
    );
}

fn run_ictus(design: &Path) -> Vec<u64> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("concat_compare_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, a: u64, b: u64| {
        sim.set("a", a);
        sim.set("b", b);
        sim.tick();
        trace.push(sim.get("flags"));
    };

    drive(&mut sim, 0x5, 0x5); // cycle 1: a==b
    drive(&mut sim, 0x3, 0x7); // cycle 2: a<b
    drive(&mut sim, 0x9, 0x2); // cycle 3: a>b

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<u64> {
    let out_dir = std::env::temp_dir().join("ictus-differential-concat-compare");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("concat_compare_test_tb.vvp");

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
