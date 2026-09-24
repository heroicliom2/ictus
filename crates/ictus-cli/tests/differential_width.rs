//! Differential test for Verilog's self-determined width rule on a
//! binary bitwise or arithmetic result -- the widths `expr_width` now
//! reports, checked against a real simulator rather than against my own
//! reading of the LRM.
//!
//! The arithmetic cases are the ones worth the trouble. `a + b` with
//! 4-bit operands is a 4-bit result, so `{a + b, 2'b11}` *discards the
//! carry*: with a = 12 and b = 10 the sum is 22, which packs as 6, not
//! 22. That is Verilog's rule and a genuine trap, so it is pinned down
//! here rather than assumed. The bitwise cases can't lose anything --
//! neither operand can set a bit above its own width -- and are here to
//! show the common shape works, including picorv32's own
//! `|(irq_pending & ~irq_mask)`, which is a *reduction* over a bitwise
//! AND and needs the same width for a different reason.
//!
//! See docs/decisions.md D24.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn width_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("width_test.v");
    let testbench = fixtures.join("width_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on width_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (any_masked, concat_and, concat_add, concat_mixed, not_sum)
            //
            // a=12, b=10: ~b is 5, a & ~b is 4, so the reduction is 1.
            // a & b is 8 -> {8, 2'b11} = 35. a + b is 22, truncated to
            // 4 bits = 6 -> {6, 2'b11} = 27. wide + a = 212 in 8 bits
            // -> {212, 2'b11} = 851. ~6 in 4 bits = 9.
            (1, 35, 27, 851, 9),
            // a=b=15: ~b is 0, so nothing survives the mask. a + b = 30
            // truncates to 14; wide + a = 270 truncates to 14 as well.
            (0, 63, 59, 59, 1),
            (0, 3, 3, 3, 15),
            // a=5, b=3: a + b = 8 fits, no truncation.
            (1, 7, 35, 423, 7),
        ],
        "expected self-determined widths, with the arithmetic carry discarded"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64, u64, u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("width_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    for (a, b, wide) in [(12, 10, 200), (15, 15, 255), (0, 0, 0), (5, 3, 100)] {
        sim.set("a", a);
        sim.set("b", b);
        sim.set("wide", wide);
        sim.tick();
        trace.push((
            sim.get("any_masked"),
            sim.get("concat_and"),
            sim.get("concat_add"),
            sim.get("concat_mixed"),
            sim.get("not_sum"),
        ));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64, u64, u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-width");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("width_test_tb.vvp");

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
