use ictus_ir::Stmt;
use std::path::Path;

/// `always @*` lowers to a `CombProcess` -- a separate list from the
/// clocked ones, with no sensitivity list, since all three spellings
/// (`always @*`, `always @(*)`, `always_comb`) mean "re-run on everything
/// this block reads". The body needs no new statement machinery: these
/// blocks are `if`/`case` plus blocking assignment, all of which already
/// existed. See docs/decisions.md D26.
#[test]
fn lowers_combinational_always_blocks() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comb_always_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("comb_always_test.v should lower cleanly");

    assert_eq!(module.comb_processes.len(), 3, "three `always @*` blocks");
    assert_eq!(module.clocked_processes.len(), 1, "one clocked block");

    // The first block is the default-then-override idiom: two plain
    // assignments, then a case that overrides them.
    let first = &module.comb_processes[0].body;
    assert!(
        matches!(first[0], Stmt::BlockingAssign { .. }),
        "expected a blocking default assignment, got {:?}",
        first[0]
    );
    assert!(
        matches!(first[2], Stmt::Case { .. }),
        "expected a case statement, got {:?}",
        first[2]
    );

    // A block that assigns only under an `if` -- Verilog's inferred
    // latch. Nothing marks it as special; leaving the previous value in
    // place is simply what running these statements does.
    let held = &module.comb_processes[2].body;
    assert!(
        matches!(held[0], Stmt::If { .. }),
        "expected the conditional assignment, got {:?}",
        held[0]
    );
}

/// A non-blocking assignment inside a combinational block is rejected.
/// It's legal Verilog and means something specific -- the write defers
/// past the block's own later statements -- and combinational settling
/// happens outside any clock edge, with no deferral phase to put it in.
/// Treating it as blocking would be a silent wrong answer.
#[test]
fn rejects_nonblocking_assignment_in_a_combinational_block() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comb_nonblocking_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("comb_nonblocking_test.v uses `<=` in an `always @*`, which v1 must reject");
    assert!(
        err.contains("non-blocking"),
        "expected the error to name the non-blocking assignment, got: {err}"
    );
}

/// An *explicit* sensitivity list is rejected rather than quietly widened
/// to `@*`. An incomplete list is a classic Verilog bug, and a simulator
/// that honours the list as written produces different results from one
/// that doesn't -- so guessing here would mean disagreeing with the
/// reference simulator on exactly the designs where it matters.
#[test]
fn rejects_an_explicit_sensitivity_list() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comb_sensitivity_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("comb_sensitivity_test.v has an explicit sensitivity list, which v1 rejects");
    assert!(
        err.contains("sensitivity list"),
        "expected the error to name the sensitivity list, got: {err}"
    );
}

/// A `negedge` block is rejected too. It used to be *skipped silently*,
/// which left its targets reading 0 forever while the design still
/// lowered and ran -- the failure mode docs/decisions.md D25 was written
/// about. Every `always` block now lowers to something or errors.
#[test]
fn rejects_a_negedge_block() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/negedge_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("negedge_test.v is negedge-triggered, which v1 must reject rather than skip");
    assert!(
        err.contains("negedge"),
        "expected the error to name the edge, got: {err}"
    );
}
