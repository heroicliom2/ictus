use ictus_ir::{Expr, Stmt};
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

    // Arm 0: `3'd0: result <= 8'h11;`
    assert_eq!(arms[0].values.len(), 1);
    assert!(matches!(arms[0].values[0], Expr::Literal { value: 0, width: 3 }));
    assert_target_value(&arms[0].body, result, 0x11);

    // Arm 2: `3'd2, 3'd3: result <= 8'h33;` -- the comma-joined multi-value arm.
    assert_eq!(arms[2].values.len(), 2, "expected the comma-joined arm to carry both values");
    assert!(matches!(arms[2].values[0], Expr::Literal { value: 2, width: 3 }));
    assert!(matches!(arms[2].values[1], Expr::Literal { value: 3, width: 3 }));
    assert_target_value(&arms[2].body, result, 0x33);

    // default: result <= 8'hFF;
    assert_target_value(default, result, 0xFF);
}

/// `casez`'s wildcard bits (`?`) would silently be treated as literal 0/1
/// if this rejection ever regressed -- that's a silent-wrong-match bug,
/// not a crash, so it's worth confirming the rejection actually fires
/// rather than trusting the code that's supposed to produce it.
#[test]
fn rejects_casez() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/casez_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("casez_test.v uses casez, which v1 must reject rather than mis-lower");
    assert!(
        err.contains("casez"),
        "expected the error to mention casez, got: {err}"
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
