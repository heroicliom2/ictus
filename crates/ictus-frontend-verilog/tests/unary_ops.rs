use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `~x`, `&x`, `|x`, `^x`, `~&x`, `~|x`, `~^x` all lower to the correct
/// `Expr` shape, each carrying `x`'s own width (4, for `[3:0] x`) --
/// the NAND/NOR/XNOR forms as `BitwiseNot` wrapping the corresponding
/// reduction, not their own IR variant (see `ictus_ir::Expr::ReduceAnd`'s
/// doc comment). `r_nand <= ~&x;` mirrors picorv32's own style directly
/// (`~&mem_rdata_latched[1:0]`).
#[test]
fn lowers_unary_bitwise_and_reduction_operators() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unary_ops_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("unary_ops_test.v should lower cleanly");

    let x = module.signal_id("x").expect("x port");
    let r_bitnot = module.signal_id("r_bitnot").expect("r_bitnot port");
    let r_and = module.signal_id("r_and").expect("r_and port");
    let r_or = module.signal_id("r_or").expect("r_or port");
    let r_xor = module.signal_id("r_xor").expect("r_xor port");
    let r_nand = module.signal_id("r_nand").expect("r_nand port");
    let r_nor = module.signal_id("r_nor").expect("r_nor port");
    let r_xnor = module.signal_id("r_xnor").expect("r_xnor port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    match find_assign(else_branch, r_bitnot) {
        Expr::BitwiseNot(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
        other => panic!("expected `~x` as BitwiseNot(Ref(x), 4), got {other:?}"),
    }
    match find_assign(else_branch, r_and) {
        Expr::ReduceAnd(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
        other => panic!("expected `&x` as ReduceAnd(Ref(x), 4), got {other:?}"),
    }
    match find_assign(else_branch, r_or) {
        Expr::ReduceOr(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
        other => panic!("expected `|x` as ReduceOr(Ref(x), 4), got {other:?}"),
    }
    match find_assign(else_branch, r_xor) {
        Expr::ReduceXor(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
        other => panic!("expected `^x` as ReduceXor(Ref(x), 4), got {other:?}"),
    }
    match find_assign(else_branch, r_nand) {
        Expr::BitwiseNot(inner, 1) => match &**inner {
            Expr::ReduceAnd(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
            other => panic!("expected ReduceAnd(Ref(x), 4) inside the NAND's BitwiseNot, got {other:?}"),
        },
        other => panic!("expected `~&x` as BitwiseNot(ReduceAnd(Ref(x), 4), 1), got {other:?}"),
    }
    match find_assign(else_branch, r_nor) {
        Expr::BitwiseNot(inner, 1) => match &**inner {
            Expr::ReduceOr(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
            other => panic!("expected ReduceOr(Ref(x), 4) inside the NOR's BitwiseNot, got {other:?}"),
        },
        other => panic!("expected `~|x` as BitwiseNot(ReduceOr(Ref(x), 4), 1), got {other:?}"),
    }
    match find_assign(else_branch, r_xnor) {
        Expr::BitwiseNot(inner, 1) => match &**inner {
            Expr::ReduceXor(inner, 4) => assert!(matches!(**inner, Expr::Ref(id) if id == x)),
            other => panic!("expected ReduceXor(Ref(x), 4) inside the XNOR's BitwiseNot, got {other:?}"),
        },
        other => panic!("expected `~^x` as BitwiseNot(ReduceXor(Ref(x), 4), 1), got {other:?}"),
    }
}

fn find_assign(stmts: &[Stmt], target: ictus_ir::SignalId) -> &Expr {
    stmts
        .iter()
        .find_map(|s| match s {
            Stmt::NonBlockingAssign { target: t, value, .. } if *t == target => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no assignment found for signal id {target}"))
}
