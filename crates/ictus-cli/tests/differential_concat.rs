//! Differential test for concatenation (`{a, b}`), including a literal
//! mixed with signal references in the same concatenation.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn concat_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("concat_test.v");
    let testbench = fixtures.join("concat_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on concat_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![(0, 0), (0xAB, 0x1AB), (0xF0, 0x1F0), (0x0F, 0x10F)],
        "expected reset, then {{hi,lo}} and {{1'b1,hi,lo}} for each (hi, lo) pair"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("concat_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, hi: u64, lo: u64| {
        sim.set("resetn", resetn);
        sim.set("hi", hi);
        sim.set("lo", lo);
        sim.tick();
        trace.push((sim.get("combined"), sim.get("with_flag")));
    };

    drive(&mut sim, 0, 0x0, 0x0); // cycle 1: reset
    drive(&mut sim, 1, 0xA, 0xB); // cycle 2
    drive(&mut sim, 1, 0xF, 0x0); // cycle 3
    drive(&mut sim, 1, 0x0, 0xF); // cycle 4

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-concat");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("concat_test_tb.vvp");

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
            Some((a, b))
        })
        .collect()
}
