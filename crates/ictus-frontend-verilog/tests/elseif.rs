use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `else if` lowers to nested `Stmt::If`s with no new IR variant -- this
/// asserts the actual nesting shape (each `else if`'s condition and body
/// landing one level deeper than the last, in source order) rather than
/// just that the design lowers without error.
#[test]
fn lowers_else_if_chain_as_nested_if() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/elseif_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("elseif_test.v should lower cleanly");

    let level = module.signal_id("level").expect("level port");
    let result = module.signal_id("result").expect("result port");

    // Outer: if (!resetn) ... else <the else-if chain>
    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    // else if (level == 0) result <= 8'h11;
    let Stmt::If {
        cond: cond0,
        then_branch: then0,
        else_branch: else0,
    } = &else_branch[0]
    else {
        panic!("expected the reset-else branch to be the first `else if`");
    };
    assert_eq_literal(cond0, level, 0);
    assert_target_value(then0, result, 0x11);

    // else if (level == 1) result <= 8'h22;
    let Stmt::If {
        cond: cond1,
        then_branch: then1,
        else_branch: else1,
    } = &else0[0]
    else {
        panic!("expected the second `else if` nested inside the first's else branch");
    };
    assert_eq_literal(cond1, level, 1);
    assert_target_value(then1, result, 0x22);

    // else if (level == 2) result <= 8'h33;
    let Stmt::If {
        cond: cond2,
        then_branch: then2,
        else_branch: else2,
    } = &else1[0]
    else {
        panic!("expected the third `else if` nested inside the second's else branch");
    };
    assert_eq_literal(cond2, level, 2);
    assert_target_value(then2, result, 0x33);

    // else result <= 8'hFF; -- the final plain `else`, at the bottom of the nesting.
    assert_target_value(else2, result, 0xFF);
}

fn assert_eq_literal(cond: &Expr, expected_ref: ictus_ir::SignalId, expected_value: u64) {
    match cond {
        Expr::Eq(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == expected_ref));
            assert!(matches!(**rhs, Expr::Literal { value, .. } if value == expected_value));
        }
        other => panic!("expected `level == {expected_value}`, got {other:?}"),
    }
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
