//! Differential test for Verilog's expression widths and signedness --
//! IEEE 1800 §11.6 and §11.8.1 -- which Ictus used to get wrong in a
//! whole family of silent ways (docs/decisions.md D30, D31).
//!
//! **The defect this pins down.** The kernel evaluates on 64-bit words
//! and masks a value only when it is written. That is right for operators
//! whose low bits depend only on their operands' low bits -- add,
//! subtract, multiply, left shift, bitwise -- and was wrong for anything
//! reading high bits -- a right shift, a comparison, equality, `&&`, `!`, a
//! shift amount -- applied to arithmetic that wrapped: with 8-bit
//! operands `(3 - 5) >> 1` is 127 in Verilog and came out 255. Separately,
//! `~a` into a wider target has to invert the *extended* operand, and a
//! `case` compares its selector and items at the width of the widest --
//! with narrower `casez` items zero-extended, not padded with wildcards.
//!
//! **The fix** is a pass in the frontend (`ictus-frontend-verilog`'s
//! `width` module) that sizes every expression top-down by its context and
//! cuts the few operators that can overflow back to the width they are
//! evaluated at. This test is the evidence it matches Icarus across every
//! form found broken, plus the three the design had to get right on
//! purpose: `{cout, sum} <= a + b` keeping its carry, a `case` selector
//! keeping one because an item is wider, and `casez` zero-extension.
//!
//! Until the fix landed this file was a preserved reproduction, marked
//! `#[ignore]`.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

const OUTPUTS: [&str; 18] = [
    "shifted",
    "below_max",
    "neg_shifted",
    "wrapped_eq",
    "add_gt",
    "add_land",
    "add_not",
    "shamt",
    "not16",
    "subshr16",
    "signed_add16",
    "neg_signed16",
    "shl_cat",
    "cout",
    "sum",
    "case_carry",
    "casez_wide",
    "casez_short",
];

const VECTORS: [(u64, u64); 10] = [
    (3, 5),
    (9, 4),
    (0, 1),
    (201, 100),
    (255, 1),
    (128, 128),
    (0, 255),
    (200, 100),
    (0, 196),
    (3, 200),
];

#[test]
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

    for (row, ((ictus, icarus), (a, b))) in ictus_trace
        .iter()
        .zip(&icarus_trace)
        .zip(VECTORS)
        .enumerate()
    {
        for (i, name) in OUTPUTS.iter().enumerate() {
            assert_eq!(
                ictus[i], icarus[i],
                "row {row} (a = {a}, b = {b}): `{name}` is {} in Ictus but {} in Icarus",
                ictus[i], icarus[i]
            );
        }
    }
    assert_eq!(ictus_trace.len(), icarus_trace.len(), "row counts differ");

    // Icarus's own answers, pinned: the cases the fix exists for. Looked
    // up by name, so adding an output can't shift what is checked.
    let at = |row: usize, name: &str| {
        let column = OUTPUTS.iter().position(|o| *o == name).expect("known output");
        ictus_trace[row][column]
    };
    assert_eq!(at(0, "shifted"), 127, "(a - b) >> 1 wraps at 8 bits before shifting");
    assert_eq!(at(0, "not16"), 65532, "~a into 16 bits inverts the extended operand");
    assert_eq!(at(0, "subshr16"), 32767, "(a - b) >> 1 into 16 bits subtracts at 16 bits");
    assert_eq!(at(3, "neg_signed16"), 55, "-$signed(a) of -55 is +55");
    assert_eq!((at(3, "cout"), at(3, "sum")), (1, 45), "{{cout, sum}} <= a + b keeps the carry");
    assert_eq!(at(7, "case_carry"), 1, "case (a + b) against a 9-bit item keeps the carry");
    assert_eq!(at(8, "casez_wide"), 1, "casez: top byte zero, so the narrow item matches");
    assert_eq!(at(9, "casez_wide"), 0, "casez: top byte non-zero, so it doesn't");
    assert_eq!(at(8, "casez_short"), 0, "casez 8'b1?? needs bits 3-7 of 196 to be zero");
}

fn run_ictus(design: &Path) -> Vec<Vec<u64>> {
    let module =
        ictus_frontend_verilog::lower_file(design).expect("width_context_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    for (a, b) in VECTORS {
        sim.set_all(&[("a", a), ("b", b)]);
        sim.tick();
        trace.push(OUTPUTS.iter().map(|name| sim.get(name)).collect());
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Vec<u64>> {
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
            let row: Option<Vec<u64>> = line
                .split_whitespace()
                .map(|f| f.parse::<u64>().ok())
                .collect();
            row.filter(|r| r.len() == OUTPUTS.len())
        })
        .collect()
}
