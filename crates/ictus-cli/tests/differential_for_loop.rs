//! Differential test for `for` loops, which Ictus unrolls while lowering,
//! together with the indexed part-selects (`x[j +: 4]`, `x[k -: 4]`) and
//! `$unsigned` that picorv32's multiplier uses alongside them. See
//! docs/decisions.md D33.
//!
//! Unrolling is only right if every copy of the body sees the loop
//! variable's value *for that iteration* -- in an index, a computed base,
//! a condition, and as a plain value -- and if iterations that read what an
//! earlier one wrote see it in order. The fixture covers each of those,
//! plus nested loops, a descending loop, and a loop in a clocked block
//! with non-blocking writes. The expected rows come from a separate
//! Python model of the same logic, so a failure says which side is wrong.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = Vec<u64>;

const VECTORS: [(u64, u64); 8] = [
    (0x00, 0x00),
    (0xFF, 0xFF),
    (0xA5, 0x3C),
    (0x01, 0x80),
    (0x96, 0x69),
    (0x7E, 0x18),
    (0x80, 0x01),
    (0x5A, 0x5A),
];

const OUTPUTS: [&str; 12] = [
    "reversed",
    "popcount",
    "nibble_swap",
    "nibble_not",
    "prefix_xor",
    "pair_matches",
    "index_sum",
    "acc",
    "ext_signed",
    "ext_unsigned",
    "carry_dropped",
    "carry_kept",
];

#[test]
fn for_loop_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("for_loop_test.v");
    let testbench = fixtures.join("for_loop_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on for_loop_test's output trace \
         (columns: a, b, then {OUTPUTS:?})"
    );
    let expected: Vec<Row> = [
        // a, b, reversed, popcount, nibble_swap, nibble_not, prefix_xor,
        // pair_matches, index_sum, acc, ext_signed, ext_unsigned,
        // carry_dropped, carry_kept
        [0, 0, 0, 0, 0, 65535, 0, 16, 0, 0, 0, 0, 0, 0],
        [
            255, 255, 255, 8, 65535, 0, 85, 16, 28, 255, 255, 15, 254, 510,
        ],
        [165, 60, 165, 4, 50010, 23235, 99, 0, 14, 80, 5, 5, 225, 225],
        [1, 128, 128, 1, 2064, 65151, 255, 9, 0, 81, 1, 1, 129, 129],
        [
            150, 105, 105, 4, 38505, 27030, 114, 8, 14, 231, 6, 6, 255, 255,
        ],
        [
            126, 24, 126, 6, 33255, 33255, 42, 2, 21, 17, 254, 14, 150, 150,
        ],
        [128, 1, 1, 1, 4104, 32766, 128, 9, 7, 145, 0, 0, 129, 129],
        [
            90, 90, 90, 4, 42405, 42405, 54, 8, 14, 235, 250, 10, 180, 180,
        ],
    ]
    .iter()
    .map(|row| row.to_vec())
    .collect();
    assert_eq!(
        ictus_trace, expected,
        "Ictus agrees with Icarus but not with the reference model"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module = ictus_frontend_verilog::lower_file(design).expect("for_loop_test.v should lower");
    let mut sim = Simulation::new(&module);

    // Reset, unsampled -- as in the testbench.
    sim.set_all(&[("rst", 1), ("a", 0), ("b", 0)]);
    sim.tick();

    let mut trace = Vec::new();
    for (a, b) in VECTORS {
        sim.set_all(&[("rst", 0), ("a", a), ("b", b)]);
        sim.tick();
        let mut row = vec![a, b];
        row.extend(OUTPUTS.iter().map(|name| sim.get(name)));
        trace.push(row);
    }
    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-for-loop");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("for_loop_test_tb.vvp");

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
            let row: Row = line
                .split_whitespace()
                .map(|s| s.parse().ok())
                .collect::<Option<_>>()?;
            (row.len() == OUTPUTS.len() + 2).then_some(row)
        })
        .collect()
}
