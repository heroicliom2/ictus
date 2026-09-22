//! Differential test for a concatenation used as a non-blocking
//! assignment target, where the parts are constant bit-selects/
//! part-selects of the same signal mixed with a plain full-width signal
//! -- mirrors picorv32's own style directly
//! (`{mem_rdata_q[31:25], mem_rdata_q[11:7]} <= {...};`). See
//! ictus-frontend-verilog's lower_concat_target_assign.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn concat_target_select_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("concat_target_select_test.v");
    let testbench = fixtures.join("concat_target_select_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on concat_target_select_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![(0, 0), (1, 0xBA), (1, 0x43), (1, 0xFF), (1, 0xF0)],
        "expected reset, then carry pinned to 1 and acc nibble-swapped from data each cycle"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design)
        .expect("concat_target_select_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, resetn: u64, data: u64| {
        sim.set("resetn", resetn);
        sim.set("data", data);
        sim.tick();
        trace.push((sim.get("carry"), sim.get("acc")));
    };

    drive(&mut sim, 0, 0x00); // cycle 1: reset
    drive(&mut sim, 1, 0xAB); // cycle 2
    drive(&mut sim, 1, 0x34); // cycle 3
    drive(&mut sim, 1, 0xFF); // cycle 4
    drive(&mut sim, 1, 0x0F); // cycle 5

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-concat-target-select");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("concat_target_select_test_tb.vvp");

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
            let carry = fields.next()?.parse::<u64>().ok()?;
            let acc = fields.next()?.parse::<u64>().ok()?;
            Some((carry, acc))
        })
        .collect()
}
