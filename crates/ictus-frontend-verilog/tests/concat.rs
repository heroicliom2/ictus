use ictus_ir::{Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_concatenation_expressions() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concat_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("concat_test.v should lower cleanly");

    let hi = module.signal_id("hi").expect("hi port");
    let lo = module.signal_id("lo").expect("lo port");
    let combined = module.signal_id("combined").expect("combined port");
    let with_flag = module.signal_id("with_flag").expect("with_flag port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    // combined <= {hi, lo};  -- MSB-first: hi then lo, 4+4 = 8 bits total.
    match find_assign(else_branch, combined) {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], (Expr::Ref(id), 4) if *id == hi));
            assert!(matches!(&parts[1], (Expr::Ref(id), 4) if *id == lo));
        }
        other => panic!("expected `{{hi, lo}}`, got {other:?}"),
    }

    // with_flag <= {1'b1, hi, lo};  -- a literal part mixed with signal refs, 1+4+4 = 9 bits.
    match find_assign(else_branch, with_flag) {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 3);
            assert!(matches!(
                &parts[0],
                (Expr::Literal { value: 1, width: 1 }, 1)
            ));
            assert!(matches!(&parts[1], (Expr::Ref(id), 4) if *id == hi));
            assert!(matches!(&parts[2], (Expr::Ref(id), 4) if *id == lo));
        }
        other => panic!("expected `{{1'b1, hi, lo}}`, got {other:?}"),
    }
}

/// A concatenation assignment target (`{a, b} <= x;`) must be rejected,
/// not silently lowered as a write to just `a` -- before this rejection
/// existed, the identifier search used to find the assignment target
/// would deep-search *past* the concatenation and find `a` alone (the
/// first identifier in source order), silently discarding `b` and the
/// split-assignment semantics entirely. Confirms that's actually rejected
/// now, for both non-blocking and continuous assignment targets.
#[test]
fn rejects_concatenation_as_assignment_target() {
    let nonblocking = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/concat_nonblocking_target_test.v");
    let err = ictus_frontend_verilog::lower_file(&nonblocking).expect_err(
        "concat_nonblocking_target_test.v assigns to a concatenation, which v1 must reject",
    );
    assert!(
        err.contains("concatenation"),
        "expected the error to mention concatenation, got: {err}"
    );

    let continuous =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concat_assign_target_test.v");
    let err = ictus_frontend_verilog::lower_file(&continuous).expect_err(
        "concat_assign_target_test.v assigns to a concatenation, which v1 must reject",
    );
    assert!(
        err.contains("concatenation"),
        "expected the error to mention concatenation, got: {err}"
    );
}

fn find_assign(stmts: &[Stmt], target: ictus_ir::SignalId) -> &Expr {
    stmts
        .iter()
        .find_map(|s| match s {
            Stmt::NonBlockingAssign { target: t, value } if *t == target => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no assignment found for signal id {target}"))
}
