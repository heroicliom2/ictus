//! Differential test for *blocking* assignment (`=`) mixed with
//! non-blocking (`<=`) in one clocked block -- picorv32's own style.
//!
//! This is the test that actually pins down the semantics, because the
//! two kinds differ only in *when* the write becomes visible, which no
//! amount of structural checking can show. Three things are compared:
//!
//!   * `chained` -- a blocking write read by the very next statement, so
//!     it must see the *new* value (deferring it would give one less).
//!   * `nb_sees_blocking` -- a non-blocking right-hand side reading a
//!     signal a blocking write just changed: also the new value.
//!   * `blocking_sees_old_nb` -- a blocking read of a signal a
//!     non-blocking write targeted two lines earlier: this must see the
//!     *pre-edge* value. It is the one that fails if blocking and
//!     non-blocking writes are collapsed into a single discipline.
//!
//! The first cycle is an unsampled warm-up: `blocking_sees_old_nb` reads
//! `nb_target` before anything has written it, which is 'x' in Icarus and
//! 0 here (decisions.md D6/D19). See docs/decisions.md D23.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn blocking_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("blocking_test.v");
    let testbench = fixtures.join("blocking_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on blocking_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (chained, nb_sees_blocking, blocking_sees_old_nb, nb_target)
            //  = in+2    = in+1            = previous nb_target  = 99
            (12, 11, 99, 99),  // in = 10
            (22, 21, 99, 99),  // in = 20
            (32, 31, 99, 99),  // in = 30
            (2, 1, 99, 99),    // in = 0
            (1, 0, 99, 99),    // in = 255: b wraps to 0, so chained is 1
        ],
        "expected the blocking writes to be visible immediately and the \
         non-blocking one only after the edge"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64, u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("blocking_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    // Warm-up, not sampled -- see this file's opening comment.
    sim.set("in", 10);
    sim.tick();

    for input in [10, 20, 30, 0, 255] {
        sim.set("in", input);
        sim.tick();
        trace.push((
            sim.get("chained"),
            sim.get("nb_sees_blocking"),
            sim.get("blocking_sees_old_nb"),
            sim.get("nb_target"),
        ));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64, u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-blocking");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("blocking_test_tb.vvp");

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
            Some((a, b, c, d))
        })
        .collect()
}
