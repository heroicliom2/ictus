//! Differential test for array (memory) signals -- picorv32's own
//! register-file shape: `reg [7:0] mem [0:3];` written at a runtime index
//! (`mem[waddr] <= wdata;`) and read at one (`mem[raddr]`), plus a
//! constant-index read that must also be an *element* read rather than a
//! bit-select.
//!
//! The array is filled during an unsampled warm-up phase: an element that
//! has never been written reads as 4-state 'x' in Icarus but as 0 in this
//! 2-state kernel (decisions.md D6/D19), so comparing one would be
//! comparing two different-but-both-valid answers rather than testing
//! anything. Every sampled cycle reads only already-written elements.
//!
//! The sampled sequence rewrites an element *while reading it*, which is
//! what checks the non-blocking timing: the read must see the pre-edge
//! contents, not the value being written in the same edge. It then reads
//! a neighbouring element, so a bug that wrote the wrong slot (or
//! clobbered the whole array) would show up rather than passing. See
//! ictus_ir::Expr::ArrayRead and ictus_ir::Stmt::ArrayAssign.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn array_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("array_test.v");
    let testbench = fixtures.join("array_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on array_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (rdata, fixed_read) -- fixed_read is the constant-index
            // read of mem[2], 0xC3 until it's overwritten at the end.
            (0xA1, 0xC3), // each element reads back as written
            (0xB2, 0xC3),
            (0xC3, 0xC3),
            (0xD4, 0xC3),
            (0xB2, 0xC3), // rewriting mem[1]: the read still sees pre-edge B2
            (0x5E, 0xC3), // now the new value
            (0xA1, 0xC3), // and mem[0] was untouched by that write
            (0xC3, 0xC3), // rewriting mem[2]: both reads see pre-edge C3
            (0x7F, 0x7F), // and both see the new value after
        ],
        "expected per-element reads, with a write visible only on the cycle after it lands"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("array_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let drive = |sim: &mut Simulation,
                 resetn: u64,
                 write_enable: u64,
                 waddr: u64,
                 wdata: u64,
                 raddr: u64| {
        sim.set("resetn", resetn);
        sim.set("write_enable", write_enable);
        sim.set("waddr", waddr);
        sim.set("wdata", wdata);
        sim.set("raddr", raddr);
        sim.tick();
    };

    // Warm-up: reset, then fill every element. Not sampled -- see this
    // file's opening comment.
    drive(&mut sim, 0, 0, 0, 0x00, 0);
    drive(&mut sim, 1, 1, 0, 0xA1, 0);
    drive(&mut sim, 1, 1, 1, 0xB2, 0);
    drive(&mut sim, 1, 1, 2, 0xC3, 0);
    drive(&mut sim, 1, 1, 3, 0xD4, 0);

    let mut sample = |sim: &mut Simulation,
                      write_enable: u64,
                      waddr: u64,
                      wdata: u64,
                      raddr: u64| {
        drive(sim, 1, write_enable, waddr, wdata, raddr);
        trace.push((sim.get("rdata"), sim.get("fixed_read")));
    };

    sample(&mut sim, 0, 3, 0xD4, 0);
    sample(&mut sim, 0, 3, 0xD4, 1);
    sample(&mut sim, 0, 3, 0xD4, 2);
    sample(&mut sim, 0, 3, 0xD4, 3);
    sample(&mut sim, 1, 1, 0x5E, 1);
    sample(&mut sim, 0, 1, 0x5E, 1);
    sample(&mut sim, 0, 1, 0x5E, 0);
    sample(&mut sim, 1, 2, 0x7F, 2);
    sample(&mut sim, 0, 2, 0x7F, 2);

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-array");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("array_test_tb.vvp");

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
