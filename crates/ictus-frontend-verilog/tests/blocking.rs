use ictus_ir::Stmt;
use std::path::Path;

/// A *blocking* assignment (`=`) inside a clocked block lowers to its own
/// statement kind, distinct from the non-blocking `<=` that sits beside
/// it in the same block. Mirrors picorv32, which mixes the two freely --
/// `set_mem_do_rinst = 1;` a few lines from `decoder_trigger <= 0;`.
///
/// The difference is *when* the write lands, which the differential test
/// in ictus-cli checks. What this test pins down is that the two kinds
/// stay distinguishable all the way through lowering, in source order:
/// collapsing them into one statement kind here would make the timing
/// unrecoverable later. See docs/decisions.md D23.
#[test]
fn lowers_blocking_and_nonblocking_assignments_side_by_side() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/blocking_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("blocking_test.v should lower cleanly");

    let b = module.signal_id("b").expect("internal b register");
    let chained = module.signal_id("chained").expect("chained port");
    let nb_sees_blocking = module
        .signal_id("nb_sees_blocking")
        .expect("nb_sees_blocking port");
    let nb_target = module.signal_id("nb_target").expect("nb_target port");
    let blocking_sees_old_nb = module
        .signal_id("blocking_sees_old_nb")
        .expect("blocking_sees_old_nb port");

    let body = &module.clocked_processes[0].body;
    assert_eq!(body.len(), 5, "expected five statements, got {body:?}");

    // Source order is preserved, and each statement keeps its own kind:
    // a blocking write is only correct *relative to* the statements
    // around it, so the order is part of the meaning here.
    let kinds: Vec<(&'static str, ictus_ir::SignalId)> = body
        .iter()
        .map(|stmt| match stmt {
            Stmt::BlockingAssign { target, target_range, .. } => {
                assert_eq!(*target_range, None, "no bit range in this fixture");
                ("blocking", *target)
            }
            Stmt::NonBlockingAssign { target, .. } => ("nonblocking", *target),
            other => panic!("unexpected statement {other:?}"),
        })
        .collect();

    assert_eq!(
        kinds,
        vec![
            ("blocking", b),
            ("blocking", chained),
            ("nonblocking", nb_sees_blocking),
            ("nonblocking", nb_target),
            ("blocking", blocking_sees_old_nb),
        ]
    );
}

/// A compound assignment (`acc += in;`) arrives through the same grammar
/// node as `=`, separated only by which operator symbol it carries. It's
/// rejected rather than quietly treated as a plain `=`, which would drop
/// the accumulate and silently compute the wrong value.
#[test]
fn rejects_compound_assignment() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/blocking_compound_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("blocking_compound_test.v uses `+=`, which v1 must reject");
    assert!(
        err.contains("+="),
        "expected the error to name the compound operator, got: {err}"
    );
}

/// `{x, y} = {y, x};` -- a blocking concatenation target whose right-hand
/// side reads a signal an earlier part writes. v1 writes the parts one at
/// a time, so the second part would see the first part's new value (Ictus
/// gave `9 9` where Icarus gives `9 3`). Rejected, naming the signal.
#[test]
fn rejects_blocking_concat_target_reading_an_earlier_part() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/blocking_concat_swap_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("a blocking swap through a concatenation target must be rejected");
    assert!(
        err.contains("reads 'x'") && err.contains("earlier part"),
        "error should name the signal read after being written: {err}"
    );
}

/// The same split with the right-hand side reading only the *last* part's
/// target is accepted: nothing has been written when it is evaluated.
/// (Its values are checked against Icarus in ictus-cli's
/// `differential_blocking_concat_target.rs`.)
#[test]
fn accepts_blocking_concat_target_reading_only_the_last_part() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ictus-cli/tests/fixtures/blocking_concat_target_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("reading the last part's target is safe and should lower");
    let hi = module.signal_id("hi").expect("hi port");
    let lo = module.signal_id("lo").expect("lo port");
    let body = &module.comb_processes[0].body;
    let targets: Vec<_> = body
        .iter()
        .map(|stmt| match stmt {
            Stmt::BlockingAssign { target, .. } => *target,
            other => panic!("expected only blocking assignments, got {other:?}"),
        })
        .collect();
    assert_eq!(targets, vec![lo, hi, lo], "`lo = b;` then one statement per part, in order");
}

/// An array element as a concatenation-target part is rejected rather than
/// taken as a bit-select of the array signal.
#[test]
fn rejects_array_element_in_concat_target() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/concat_target_array_part_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("an array element in a concatenation target must be rejected");
    assert!(err.contains("mem"), "error should name the array: {err}");
}
