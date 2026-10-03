//! Differential test for declared ranges: vectors numbered from somewhere
//! other than 0 (`reg [8:1] r`), vectors numbered upwards (`reg [0:7] q`,
//! whose index 0 is the *most* significant bit), a port declared the same
//! way, and an array whose elements are numbered 1 to 4.
//!
//! The frontend used to keep only a range's width and treat every index
//! as a bit position from 0, so each of these read or wrote the wrong
//! bit, silently; before this test `r[1]` gave Icarus's answer for `r[2]`.
//! Every form of select is covered -- a constant bit, a part-select, an
//! indexed part-select, a runtime index, a write target, an array index
//! -- since each goes through its own path in the frontend. See
//! docs/decisions.md D34.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

const OUTPUTS: [&str; 12] = [
    "off_bit",
    "off_part",
    "off_dyn",
    "off_up",
    "asc_bit",
    "asc_part",
    "asc_dyn",
    "asc_up",
    "off_written",
    "asc_written",
    "mem_read",
    "port_bit",
];

/// `(d, i, p)` per edge: the unsampled warm-up first, which writes every
/// array element, then the sampled vectors.
const WARM_UP: [(u64, u64, u64); 4] = [(0x11, 0, 0), (0x22, 1, 0), (0x33, 2, 0), (0x44, 3, 0)];
const VECTORS: [(u64, u64, u64); 6] = [
    (0b1000_0001, 0, 1),
    (0b0000_0010, 1, 2),
    (0b1111_0000, 3, 3),
    (0b0101_0110, 6, 5),
    (0b1100_1010, 5, 0),
    (0b0011_1001, 2, 255),
];

#[test]
fn range_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("range_test.v");
    let testbench = fixtures.join("range_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);
    assert_eq!(ictus_trace.len(), icarus_trace.len(), "row counts differ");

    for (row, ((ictus, icarus), (d, i, p))) in ictus_trace
        .iter()
        .zip(&icarus_trace)
        .zip(VECTORS)
        .enumerate()
    {
        for (column, name) in OUTPUTS.iter().enumerate() {
            assert_eq!(
                ictus[column], icarus[column],
                "row {row} (d = {d:#010b}, i = {i}, p = {p}): `{name}` is {} in Ictus but {} \
                 in Icarus",
                ictus[column], icarus[column]
            );
        }
    }

    // Icarus's own answers for the first sampled row, d = 1000_0001,
    // pinned where the old behaviour differed: index 1 of `[8:1]` and
    // index 0 of `[0:7]` are both real bits of d, not their neighbours.
    let at = |row: usize, name: &str| {
        let column = OUTPUTS.iter().position(|o| *o == name).expect("known output");
        ictus_trace[row][column]
    };
    assert_eq!(at(0, "off_bit"), 1, "r[1] is d's least significant bit");
    assert_eq!(at(0, "asc_part"), 8, "q[0:3] is d's top nibble");
    assert_eq!(at(1, "asc_written"), 0x18, "v[0:3] <= d[3:0] fills v's top nibble");
}

fn run_ictus(design: &Path) -> Vec<Vec<u64>> {
    let module = ictus_frontend_verilog::lower_file(design).expect("range_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    for (d, i, p) in WARM_UP {
        sim.set_all(&[("d", d), ("i", i), ("p", p)]);
        sim.tick();
    }
    for (d, i, p) in VECTORS {
        sim.set_all(&[("d", d), ("i", i), ("p", p)]);
        sim.tick();
        trace.push(OUTPUTS.iter().map(|name| sim.get(name)).collect());
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Vec<u64>> {
    let out_dir = std::env::temp_dir().join("ictus-differential-range");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("range_test_tb.vvp");

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
