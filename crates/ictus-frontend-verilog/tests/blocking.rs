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
