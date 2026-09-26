//! Differential test for module instantiation. Icarus simulates
//! `instance_test.v`'s hierarchy as written -- a top with three instances
//! of `accum`, each containing a `counter` -- while Ictus flattens it into
//! one module first. Agreement is the evidence that flattening preserves
//! behaviour: aliased ports, an input driven by an expression, an output
//! truncated into a narrower signal, per-instance state, and a parameter
//! passed down two levels. See docs/decisions.md D30.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64, u64);

#[test]
fn instance_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("instance_test.v");
    let testbench = fixtures.join("instance_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on instance_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (sum_q, diff_q, narrow_q, count_a, last_sum). Each `accum`
            // adds its input into `q` every cycle; `narrow_q` is the low 4
            // bits of an 8-bit accumulator; `count_a` is the grandchild
            // counter, aliased up two levels; `last_sum` the previous input.
            (4, 2, 3, 1, 4),
            (18, 8, 13, 2, 14),
            (62, 108, 5, 3, 44),  // 200 + 100 wraps to 44; 13 + 200 = 213, low nibble 5
            (78, 106, 12, 4, 16), // 7 - 9 wraps to 254, so 108 + 254 wraps to 106
            (78, 106, 12, 5, 0),
        ],
        "expected three independent accumulators and a counter two levels down"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module = ictus_frontend_verilog::lower_file(design).expect("instance_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    // Two edges in reset, unsampled -- see the testbench.
    sim.set_all(&[("resetn", 0), ("a", 0), ("b", 0)]);
    sim.tick();
    sim.tick();

    for (a, b) in [(3u64, 1u64), (10, 4), (200, 100), (7, 9), (0, 0)] {
        sim.set_all(&[("resetn", 1), ("a", a), ("b", b)]);
        sim.tick();
        trace.push((
            sim.get("sum_q"),
            sim.get("diff_q"),
            sim.get("narrow_q"),
            sim.get("count_a"),
            sim.get("last_sum"),
        ));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-instance");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("instance_test_tb.vvp");

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
            Some((
                f.next()?.parse::<u64>().ok()?,
                f.next()?.parse::<u64>().ok()?,
                f.next()?.parse::<u64>().ok()?,
                f.next()?.parse::<u64>().ok()?,
                f.next()?.parse::<u64>().ok()?,
            ))
        })
        .collect()
}
