//! Differential test for the shift operators (`<<`, `>>`, `>>>`).
//! Values are deliberately chosen so the sign bit is set in three of the
//! four cases: an arithmetic right shift (`$signed(x) >>> n`) and a
//! logical one (`x >> n`) then give visibly different answers, so a
//! regression that silently lowered `>>>` as a logical shift would fail
//! here rather than slipping through. The fourth case (`0x0F`, sign bit
//! clear) is the control where all three right shifts must agree.
//! See ictus_ir::Expr::Shl/Shr/AShr.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

type Row = (u64, u64, u64, u64, u64);

#[test]
fn shift_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("shift_test.v");
    let testbench = fixtures.join("shift_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on shift_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // 0x80 (-128) by 2: shl drops every bit past bit 7 (0x00);
            // logical >> gives 0x20; arithmetic >>> gives 0xE0 (-32), and
            // 0xFFE0 in the 16-bit target; unsigned >>> matches the
            // logical shift.
            (0x00, 0x20, 0xE0, 0xFFE0, 0x20),
            // 0x0F (+15) by 1: sign bit clear, all right shifts agree.
            (0x1E, 0x07, 0x07, 0x0007, 0x07),
            // 0xF0 (-16) by 3.
            (0x80, 0x1E, 0xFE, 0xFFFE, 0x1E),
            // 0xFF (-1) by 7: an arithmetic shift of -1 stays -1.
            (0x80, 0x01, 0xFF, 0xFFFF, 0x01),
        ],
        "expected <<, >>, $signed >>>, $signed >>> into a wider target, and unsigned >>>"
    );
}

fn run_ictus(design: &Path) -> Vec<Row> {
    let module = ictus_frontend_verilog::lower_file(design).expect("shift_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    let mut drive = |sim: &mut Simulation, value: u64, amount: u64| {
        sim.set("value", value);
        sim.set("amount", amount);
        sim.tick();
        trace.push((
            sim.get("shl_out"),
            sim.get("shr_out"),
            sim.get("ashr_out"),
            sim.get("ashr_wide_out"),
            sim.get("ashr_unsigned_out"),
        ));
    };

    drive(&mut sim, 0x80, 2);
    drive(&mut sim, 0x0F, 1);
    drive(&mut sim, 0xF0, 3);
    drive(&mut sim, 0xFF, 7);

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<Row> {
    let out_dir = std::env::temp_dir().join("ictus-differential-shift");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("shift_test_tb.vvp");

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
            let c = fields.next()?.parse::<u64>().ok()?;
            let d = fields.next()?.parse::<u64>().ok()?;
            let e = fields.next()?.parse::<u64>().ok()?;
            Some((a, b, c, d, e))
        })
        .collect()
}
