use ictus_ir::Expr;
use std::path::Path;

#[test]
fn lowers_continuous_assigns() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comb_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("comb_test.v should lower cleanly");

    let a = module.signal_id("a").expect("a port");
    let b = module.signal_id("b").expect("b port");
    let sum_comb = module.signal_id("sum_comb").expect("sum_comb port");
    let sum_reg = module.signal_id("sum_reg").expect("sum_reg port");
    let sum_high = module.signal_id("sum_high").expect("sum_high port");

    assert_eq!(module.assigns.len(), 2, "expected two `assign` statements");

    let sum_comb_assign = module
        .assigns
        .iter()
        .find(|assign| assign.target == sum_comb)
        .expect("assign for sum_comb");
    match &sum_comb_assign.value {
        Expr::Add(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == a));
            assert!(matches!(**rhs, Expr::Ref(id) if id == b));
        }
        other => panic!("expected `a + b`, got {other:?}"),
    }

    let sum_high_assign = module
        .assigns
        .iter()
        .find(|assign| assign.target == sum_high)
        .expect("assign for sum_high");
    match &sum_high_assign.value {
        Expr::Ge(lhs, rhs) => {
            assert!(matches!(**lhs, Expr::Ref(id) if id == sum_reg));
            assert!(matches!(**rhs, Expr::Literal { value: 128, width: 8 }));
        }
        other => panic!("expected `sum_reg >= 8'd128`, got {other:?}"),
    }
}
