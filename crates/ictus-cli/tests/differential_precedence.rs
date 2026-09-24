//! Differential test for binary operator precedence.
//!
//! `sv-parser` returns a binary expression as a right-leaning chain in
//! source order, with no precedence applied at all: `a == b && c == d`
//! comes back as `a == (b && (c == d))`. Lowering that literally computes
//! a different value than Verilog specifies, and the failure is quiet --
//! picorv32's instruction decoder is built from exactly this shape, and
//! under the literal reading it decoded `addi` as a shift instruction
//! while the core kept running and producing plausible output.
//!
//! The vectors are chosen so the two readings disagree; several obvious
//! ones agree by coincidence, which is why the structural test in
//! ictus-frontend-verilog exists alongside this one. See decisions.md D25.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn precedence_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("precedence_test.v");
    let testbench = fixtures.join("precedence_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on precedence_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (decoded, left_assoc, mixed_arith, tern)
            //
            // slli: funct3 001, funct7 0000000 -> decoded. 20-6-3 = 11
            // (the parser's `20 - (6 - 3)` would give 17). 20 + 6*3 = 38.
            // 20 > 6, so tern picks b = 6.
            (1, 11, 38, 6),
            // addi: not a shift. 10-30-4 wraps to 232; 10 + 30*4 = 130.
            // 10 > 30 is false, so tern picks c = 4.
            (0, 232, 130, 4),
            // srai: funct3 101, so not the 001 pattern. 200-100-50 = 50;
            // 200 + 100*50 = 5200, truncated to 8 bits = 80. 200 > 100,
            // so tern picks b = 100 -- the one vector where the operands
            // straddle 127, which is what makes it worth having.
            (0, 50, 80, 100),
            // funct3 001 but funct7 != 0, so the AND's right arm fails.
            (0, 252, 7, 3),
            (1, 253, 0, 1),
        ],
        "expected Verilog precedence, not the parser's source-order nesting"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64, u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("precedence_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    for (d, a, b, c) in [
        (0x00109093u64, 20u64, 6u64, 3u64),
        (0x00500093, 10, 30, 4),
        (0x40505093, 200, 100, 50),
        (0x02109093, 1, 2, 3),
        (0x00109093, 255, 1, 1),
    ] {
        sim.set("d", d);
        sim.set("a", a);
        sim.set("b", b);
        sim.set("c", c);
        sim.tick();
        trace.push((
            sim.get("decoded"),
            sim.get("left_assoc"),
            sim.get("mixed_arith"),
            sim.get("tern"),
        ));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64, u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-precedence");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("precedence_test_tb.vvp");

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
