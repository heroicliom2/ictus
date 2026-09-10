use ictus_ir::{CaseValue, Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_case_statement() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/case_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("case_test.v should lower cleanly");

    let sel = module.signal_id("sel").expect("sel port");
    let result = module.signal_id("result").expect("result port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    let Stmt::Case {
        selector,
        arms,
        default,
    } = &else_branch[0]
    else {
        panic!("expected the else branch to contain the case statement");
    };

    assert!(matches!(selector, Expr::Ref(id) if *id == sel));
    assert_eq!(arms.len(), 3, "expected 3 non-default arms (0, 1, and the comma-joined 2/3)");

    // Arm 0: `3'd0: result <= 8'h11;` -- plain `case` items are always
    // CaseValue::Exact.
    assert_eq!(arms[0].values.len(), 1);
    assert!(matches!(
        &arms[0].values[0],
        CaseValue::Exact(Expr::Literal { value: 0, width: 3 })
    ));
    assert_target_value(&arms[0].body, result, 0x11);

    // Arm 2: `3'd2, 3'd3: result <= 8'h33;` -- the comma-joined multi-value arm.
    assert_eq!(arms[2].values.len(), 2, "expected the comma-joined arm to carry both values");
    assert!(matches!(
        &arms[2].values[0],
        CaseValue::Exact(Expr::Literal { value: 2, width: 3 })
    ));
    assert!(matches!(
        &arms[2].values[1],
        CaseValue::Exact(Expr::Literal { value: 3, width: 3 })
    ));
    assert_target_value(&arms[2].body, result, 0x33);

    // default: result <= 8'hFF;
    assert_target_value(default, result, 0xFF);
}

#[test]
fn lowers_casez_wildcard_and_exact_arms() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/casez_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("casez_test.v should lower cleanly");

    let Stmt::Case { arms, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to be the casez statement");
    };
    assert_eq!(arms.len(), 3);

    // `4'b1???`: value bit 3 = 1, care_mask only covers bit 3.
    match &arms[0].values[0] {
        CaseValue::Wildcard { value, care_mask } => {
            assert_eq!(*value, 0b1000);
            assert_eq!(*care_mask, 0b1000, "only the written '1' bit should be in the care mask");
        }
        other => panic!("expected a wildcard value for `4'b1???`, got {other:?}"),
    }

    // `4'b01??`: bits 3:2 = 01, care_mask covers bits 3:2 only.
    match &arms[1].values[0] {
        CaseValue::Wildcard { value, care_mask } => {
            assert_eq!(*value, 0b0100);
            assert_eq!(*care_mask, 0b1100);
        }
        other => panic!("expected a wildcard value for `4'b01??`, got {other:?}"),
    }

    // `4'd0`: no wildcard bits written -- an exact-match item mixed into
    // the same casez, which is valid and not unusual.
    assert!(matches!(
        &arms[2].values[0],
        CaseValue::Exact(Expr::Literal { value: 0, width: 4 })
    ));
}

/// Wildcard bits only make sense as a casez/casex match pattern -- this
/// design assigns one to a signal directly, which has no well-defined
/// 2-state meaning. Confirms that's still rejected (the general literal
/// parser used everywhere outside case items doesn't understand `?`/`x`/`z`
/// digits), i.e. that adding casez support didn't loosen this by accident.
#[test]
fn rejects_wildcard_literal_outside_case() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wildcard_outside_case_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("wildcard_outside_case_test.v assigns a wildcard literal, which v1 must reject");
    assert!(
        err.contains("could not parse") || err.contains("literal"),
        "expected a literal-parsing error, got: {err}"
    );
}

fn assert_target_value(body: &[Stmt], target: ictus_ir::SignalId, expected: u64) {
    assert_eq!(body.len(), 1);
    match &body[0] {
        Stmt::NonBlockingAssign { target: t, value } => {
            assert_eq!(*t, target);
            assert!(
                matches!(value, Expr::Literal { value: v, .. } if *v == expected),
                "expected literal {expected:#x}, got {value:?}"
            );
        }
        other => panic!("expected a non-blocking assignment, got {other:?}"),
    }
}
