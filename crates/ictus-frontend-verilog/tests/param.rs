use ictus_ir::{Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_module_parameters_as_literals() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/param_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("param_test.v should lower cleanly");

    // Parameters are never signals -- OFFSET/LIMIT must not show up in
    // the signal table at all, and every reference to them must have
    // been resolved directly to a Literal in the expression tree.
    assert!(module.signal_id("OFFSET").is_none());
    assert!(module.signal_id("LIMIT").is_none());

    let result = module.signal_id("result").expect("result port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    let Stmt::If {
        cond,
        then_branch,
        else_branch: else2,
    } = &else_branch[0]
    else {
        panic!("expected the reset-else branch to be `if (result >= LIMIT) ... else ...`");
    };

    // `result >= LIMIT` -- LIMIT resolved to `8'd12`.
    match cond {
        Expr::Ge(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == result));
            assert!(matches!(**rhs, Expr::Literal { value: 12, width: 8 }));
        }
        other => panic!("expected `result >= LIMIT`, got {other:?}"),
    }

    // `result <= OFFSET;` -- OFFSET resolved to `8'd5`.
    assert_eq!(then_branch.len(), 1);
    match &then_branch[0] {
        Stmt::NonBlockingAssign { target, value } => {
            assert_eq!(*target, result);
            assert!(matches!(value, Expr::Literal { value: 5, width: 8 }));
        }
        other => panic!("expected `result <= OFFSET`, got {other:?}"),
    }

    // `result <= result + OFFSET;`
    assert_eq!(else2.len(), 1);
    match &else2[0] {
        Stmt::NonBlockingAssign { target, value } => {
            assert_eq!(*target, result);
            match value {
                Expr::Add(lhs, rhs) => {
                    assert!(matches!(**lhs, Expr::Ref(id) if id == result));
                    assert!(matches!(**rhs, Expr::Literal { value: 5, width: 8 }));
                }
                other => panic!("expected `result + OFFSET`, got {other:?}"),
            }
        }
        other => panic!("expected `result <= result + OFFSET`, got {other:?}"),
    }
}
