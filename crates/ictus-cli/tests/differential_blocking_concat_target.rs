//! Differential test for *blocking* assignment to a concatenation target
//! (`{a, b} = value;`), the form picorv32's multiplier uses.
//!
//! The target is lowered as one statement per part, each writing its
//! slice of its own copy of the value. For `<=` that is exact. For `=`
//! each part is written before the next part's copy is evaluated, so a
//! right-hand side that reads an earlier part's target would see the new
//! value -- `{x, y} = {y, x};` gave `9 9` in Ictus against Icarus's `9 3`.
//! That form is now rejected (see the frontend's `blocking.rs`); this test
//! pins down that the forms still accepted -- reading the *last* part's
//! target, or neither -- agree with Icarus, carry included.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64, u64, u64);

const VECTORS: [(u64, u64); 6] = [(3, 9), (15, 15), (0, 0), (8, 7), (0, 2), (12, 5)];

#[test]
fn blocking_concat_target_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("blocking_concat_target_test.v");
    let testbench = fixtures.join("blocking_concat_target_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on blocking_concat_target_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // ({hi,lo} = {a,b}+1, {cout,sum} = a+b, {q_hi,q_lo} = {a^b,a}-3)
            (3, 10, 0, 12, 10, 0),  // a=3,  b=9
            (0, 0, 1, 14, 0, 12),   // a=15, b=15: {a,b}+1 wraps to 0
            (0, 1, 0, 0, 15, 13),   // a=0,  b=0: {0,0}-3 wraps to 0xFD
            (8, 8, 0, 15, 15, 5),   // a=8,  b=7
            (0, 3, 0, 2, 1, 13),    // a=0,  b=2
            (12, 6, 1, 1, 9, 9),    // a=12, b=5: carry out of the sum
        ],
        "expected each value split across its target with the carry kept"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module = ictus_frontend_verilog::lower_file(design)
        .expect("blocking_concat_target_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();
    for (a, b) in VECTORS {
        sim.set_all(&[("a", a), ("b", b)]);
        sim.tick();
        trace.push((
            sim.get("hi"),
            sim.get("lo"),
            sim.get("cout"),
            sim.get("sum"),
            sim.get("q_hi"),
            sim.get("q_lo"),
        ));
    }
    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-blocking-concat-target");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("blocking_concat_target_test_tb.vvp");

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
            let f: Vec<u64> = line
                .split_whitespace()
                .map(|s| s.parse().ok())
                .collect::<Option<_>>()?;
            match f.as_slice() {
                &[a, b, c, d, e, g] => Some((a, b, c, d, e, g)),
                _ => None,
            }
        })
        .collect()
}
