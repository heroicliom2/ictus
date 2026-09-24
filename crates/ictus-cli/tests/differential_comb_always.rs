//! Differential test for combinational `always @*` blocks.
//!
//! Three things are being pinned down, and each is a place this could go
//! quietly wrong rather than fail loudly:
//!
//!   * **The default-then-override idiom.** `muxed = 0;` followed by a
//!     `case` that overrides it is how combinational Verilog is normally
//!     written, and it is why settling cannot decide it has converged by
//!     asking each write whether it changed something -- every pass over
//!     such a block writes twice and can end where it began.
//!   * **Evaluation order not mattering.** `chained` reads `muxed` and is
//!     declared in a block *before* the one that computes it, so a single
//!     pass in source order would compute it from a stale value. The
//!     kernel settles to a fixpoint instead.
//!   * **An inferred latch.** `held` is assigned only when `en` is high,
//!     so it keeps its previous value otherwise. The sequence changes `a`
//!     while `en` is low, which is what makes that observable.
//!
//! See docs/decisions.md D26.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64, u64);

#[test]
fn comb_always_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("comb_always_test.v");
    let testbench = fixtures.join("comb_always_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on comb_always_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (muxed, flag, held, chained, latched). `latched` holds the
            // *same* row's `chained`, not the previous one: the inputs
            // are driven before the edge, so the combinational chain has
            // already settled by the time the register samples it.
            (11, 0, 11, 12, 12),   // sel 0 -> a
            (22, 0, 11, 23, 23),   // sel 1 -> b
            (33, 1, 11, 34, 34),   // sel 2 -> a + b, and flag overridden to 1
            (255, 0, 11, 0, 0),    // default arm; chained wraps 255 + 1 to 0
            (99, 0, 11, 100, 100), // en low: held keeps 11 though a is now 99
            (100, 1, 11, 101, 101),
            (99, 0, 99, 100, 100), // en high again: held follows a
        ],
        "expected default-then-override, order-independent settling, and a latch"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("comb_always_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    // `set_all`, not four `set` calls: the testbench drives all four in
    // one instant, and `held` is a latch, so a settle in between would
    // let it capture the new `a` before `en` goes low. See
    // `Simulation::set_all`.
    let drive = |sim: &mut Simulation, sel: u64, a: u64, b: u64, en: u64| {
        sim.set_all(&[("sel", sel), ("a", a), ("b", b), ("en", en)]);
        sim.tick();
    };

    // Warm-up, not sampled -- see the testbench.
    drive(&mut sim, 0, 11, 22, 1);

    for (sel, a, b, en) in [
        (0u64, 11u64, 22u64, 1u64),
        (1, 11, 22, 1),
        (2, 11, 22, 1),
        (3, 11, 22, 1),
        (0, 99, 22, 0),
        (2, 99, 1, 0),
        (0, 99, 1, 1),
    ] {
        drive(&mut sim, sel, a, b, en);
        trace.push((
            sim.get("muxed"),
            sim.get("flag"),
            sim.get("held"),
            sim.get("chained"),
            sim.get("latched"),
        ));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-comb-always");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("comb_always_test_tb.vvp");

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
