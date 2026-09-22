//! Differential test for replication/multiple concatenation (`{N{expr}}`)
//! -- both a single-expression replication (mirroring picorv32's own
//! `mem_la_write & {4{...}}` style) and replicating a multi-part inner
//! concatenation (`{N{a, b}}`). See ictus-frontend-verilog's
//! lower_multiple_concatenation.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn replicate_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("replicate_test.v");
    let testbench = fixtures.join("replicate_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on replicate_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![(0, 0), (0xFF, 0x99), (0x00, 0xCC), (0xFF, 0x33)],
        "expected reset, then masked = 0xFF & {{8{{flag}}}} and doubled = {{2{{a,b}}}} each cycle"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("replicate_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, a: u64, b: u64, flag: u64| {
        sim.set("resetn", resetn);
        sim.set("a", a);
        sim.set("b", b);
        sim.set("flag", flag);
        sim.tick();
        trace.push((sim.get("masked"), sim.get("doubled")));
    };

    drive(&mut sim, 0, 0b00, 0b00, 0); // cycle 1: reset
    drive(&mut sim, 1, 0b10, 0b01, 1); // cycle 2
    drive(&mut sim, 1, 0b11, 0b00, 0); // cycle 3
    drive(&mut sim, 1, 0b00, 0b11, 1); // cycle 4

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-replicate");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("replicate_test_tb.vvp");

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
            let masked = fields.next()?.parse::<u64>().ok()?;
            let doubled = fields.next()?.parse::<u64>().ok()?;
            Some((masked, doubled))
        })
        .collect()
}
