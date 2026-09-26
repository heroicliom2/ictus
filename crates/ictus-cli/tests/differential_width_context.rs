//! Differential test for Verilog's *context-determined* expression widths
//! -- and, today, a preserved reproduction of a known defect.
//!
//! **The defect.** Ictus evaluates every expression on a 64-bit word and
//! relies on masking to the target's width when the result is written.
//! That is right for operators whose low bits depend only on their
//! operands' low bits -- add, subtract, multiply, left shift, bitwise --
//! and wrong for operators that look at *high* bits -- right shifts,
//! comparisons, equality -- applied to an arithmetic result that wrapped.
//! Verilog evaluates `(a - b) >> 1` with 8-bit operands at 8 bits
//! (IEEE 1800 §11.6), so `3 - 5` wraps to 254 *before* the shift and the
//! answer is 127; Ictus shifts the 64-bit `0xFFFF_FFFF_FFFF_FFFE` and keeps
//! the low 8 bits of that, 255. Found while adding unary minus
//! (docs/decisions.md D30), which is lowered as subtraction and inherits
//! the same behaviour.
//!
//! Nothing in the existing suite catches it -- picorv32 included, since
//! its arithmetic results go straight into registers -- which is exactly
//! why it is kept here as a runnable reproduction rather than only
//! described. The fix is the next increment; it removes the `#[ignore]`.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64);

#[test]
#[ignore = "known defect: arithmetic isn't reduced to its Verilog width before a right \
            shift or comparison reads it -- see this file's header and decisions.md D30"]
fn width_context_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("width_context_test.v");
    let testbench = fixtures.join("width_context_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on width_context_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (shifted, below_max, neg_shifted, wrapped_eq)
            (127, 1, 126, 1), // 3 - 5 wraps to 254 at 8 bits; -3 is 253
            (2, 1, 123, 0),   // 9 - 4 = 5 doesn't wrap; -9 is 247
            (127, 0, 0, 0),   // 0 - 1 wraps to 255, which is not < 255
        ],
        "expected every operation evaluated at the 8-bit width Verilog gives it"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("width_context_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    for (a, b) in [(3u64, 5u64), (9, 4), (0, 1)] {
        sim.set_all(&[("a", a), ("b", b)]);
        sim.tick();
        trace.push((
            sim.get("shifted"),
            sim.get("below_max"),
            sim.get("neg_shifted"),
            sim.get("wrapped_eq"),
        ));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-width-context");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("width_context_test_tb.vvp");

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
            ))
        })
        .collect()
}
