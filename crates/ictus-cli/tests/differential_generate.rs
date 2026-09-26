//! Differential test for `generate if` elaboration.
//!
//! `generate_test.v` has two `generate if`s selecting *opposite* branches
//! -- `sum` registered, `diff` combinational -- plus an `else if` chain.
//! Samples are taken both between edges and after them. The between-edge
//! samples are the point: a registered and a combinational `a + b` agree
//! right after an edge, so the bug this guards against (lowering both
//! branches of every `generate if`, which is what the frontend used to
//! do) passes every post-edge comparison. Between edges the registered
//! `sum` still shows the previous cycle's value, and a design that also
//! contained the combinational branch would not. See decisions.md D27.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (char, u64, u64, u64);

#[test]
fn generate_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("generate_test.v");
    let testbench = fixtures.join("generate_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on generate_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (phase, sum, diff, picked). `sum` is registered, so a mid-
            // cycle ('M') sample shows the *previous* sum; `diff` and
            // `picked` are combinational and already show the new inputs.
            ('M', 3, 7, 9),      // a=10,b=3: sum still 1+2 from warm-up
            ('P', 13, 7, 9),
            ('M', 13, 100, 172), // a=200,b=100: 200^100 = 172
            ('P', 44, 100, 172), // 300 truncated to 8 bits
            ('M', 44, 252, 12),  // a=5,b=9: 5-9 wraps to 252
            ('P', 14, 252, 12),
        ],
        "expected a registered sum, a combinational diff, and the final else branch"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module = ictus_frontend_verilog::lower_file(design).expect("generate_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    // Warm-up, not sampled -- see the testbench.
    sim.set_all(&[("a", 1), ("b", 2)]);
    sim.tick();

    let sample = |phase, sim: &Simulation| (phase, sim.get("sum"), sim.get("diff"), sim.get("picked"));
    for (a, b) in [(10u64, 3u64), (200, 100), (5, 9)] {
        sim.set_all(&[("a", a), ("b", b)]);
        trace.push(sample('M', &sim));
        sim.tick();
        trace.push(sample('P', &sim));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-generate");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("generate_test_tb.vvp");

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
            let mut f = line.split_whitespace();
            let phase = f.next()?.chars().next()?;
            Some((
                phase,
                f.next()?.parse::<u64>().ok()?,
                f.next()?.parse::<u64>().ok()?,
                f.next()?.parse::<u64>().ok()?,
            ))
        })
        .collect()
}
