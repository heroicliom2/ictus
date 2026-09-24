//! Differential test for net declarations carrying an initializer
//! (`wire doubled = a + a;`), which IEEE 1800 defines as exactly
//! equivalent to a standalone continuous assignment.
//!
//! These used to be dropped silently: the wire existed and read 0
//! forever, while the design still lowered and still ran. picorv32 drives
//! most of its combinational logic this way, so the effect there was a
//! CPU whose control signals were all stuck at 0 -- which is how the
//! defect stayed invisible until the whole core was run against Icarus.
//! The fixture mixes both spellings so a regression that handled only one
//! would show up. See docs/decisions.md D25.

use ictus_kernel::Simulation;
use std::path::Path;
use std::process::Command;

#[test]
fn net_init_test_matches_icarus_verilog() {
    if Command::new("iverilog").arg("-V").output().is_err() {
        eprintln!("iverilog not found on PATH; skipping differential test");
        return;
    }

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let design = fixtures.join("net_init_test.v");
    let testbench = fixtures.join("net_init_test_tb.v");

    let ictus_trace = run_ictus(&design);
    let icarus_trace = run_icarus(&design, &testbench);

    assert_eq!(
        icarus_trace, ictus_trace,
        "Ictus and Icarus Verilog disagree on net_init_test's output trace"
    );
    assert_eq!(
        ictus_trace,
        vec![
            // (sum, both, latched). `sum` is (a + a) + b, `both` is
            // (a > b) && (b != 0), and `latched` is `doubled` sampled at
            // the edge -- which already reflects the `a` driven before
            // that edge, so it does *not* lag a cycle behind.
            (10, 0, 6),    // a=3,b=4: doubled 6, 6+4=10; 3 > 4 is false
            (22, 1, 20),   // a=10,b=2: doubled 20, 20+2=22; both true
            (20, 0, 20),   // a=10,b=0: b == 0, so `both` is false
            (144, 0, 200), // a=100,b=200: 200+200=400, truncated to 144
            (5, 0, 0),     // a=0,b=5: doubled 0, so sum is just b
        ],
        "expected the declaration-form initializers to drive their nets"
    );
}

fn run_ictus(design: &Path) -> Vec<(u64, u64, u64)> {
    let module = ictus_frontend_verilog::lower_file(design).expect("net_init_test.v should lower");
    let mut sim = Simulation::new(&module);
    let mut trace = Vec::new();

    for (a, b) in [(3u64, 4u64), (10, 2), (10, 0), (100, 200), (0, 5)] {
        sim.set("a", a);
        sim.set("b", b);
        sim.tick();
        trace.push((sim.get("sum"), sim.get("both"), sim.get("latched")));
    }

    trace
}

fn run_icarus(design: &Path, testbench: &Path) -> Vec<(u64, u64, u64)> {
    let out_dir = std::env::temp_dir().join("ictus-differential-net-init");
    std::fs::create_dir_all(&out_dir).expect("failed to create temp output dir");
    let compiled = out_dir.join("net_init_test_tb.vvp");

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
            ))
        })
        .collect()
}
