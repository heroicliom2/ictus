use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// All four signed ordering comparisons lower out of the single
/// `Expr::SignedLt` variant, by swapping operands and/or negating:
/// `a > b` is `b < a`, `a <= b` is `!(b < a)`, `a >= b` is `!(a < b)`.
/// Mirrors picorv32's own `alu_lts <= $signed(reg_op1) < $signed(reg_op2);`.
#[test]
fn lowers_signed_ordering_comparisons() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/signed_compare_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("signed_compare_test.v should lower cleanly");

    let a = module.signal_id("a").expect("a port");
    let b = module.signal_id("b").expect("b port");
    let lt = module.signal_id("lt").expect("lt port");
    let gt = module.signal_id("gt").expect("gt port");
    let le = module.signal_id("le").expect("le port");
    let ge = module.signal_id("ge").expect("ge port");
    let unsigned_lt = module.signal_id("unsigned_lt").expect("unsigned_lt port");

    let body = &module.clocked_processes[0].body;

    // `$signed(x)` wrapping a reference to the given signal, at width 8.
    let is_signed_ref = |expr: &Expr, want: ictus_ir::SignalId| match expr {
        Expr::Signed(inner, 8) => matches!(**inner, Expr::Ref(id) if id == want),
        _ => false,
    };

    // lt: $signed(a) < $signed(b)  ->  SignedLt(a, b)
    match find_assign(body, lt) {
        Expr::SignedLt(l, r) => {
            assert!(is_signed_ref(l, a) && is_signed_ref(r, b), "expected SignedLt(a, b)");
        }
        other => panic!("expected `$signed(a) < $signed(b)` as SignedLt(a, b), got {other:?}"),
    }
    // gt: $signed(a) > $signed(b)  ->  SignedLt(b, a), operands swapped
    match find_assign(body, gt) {
        Expr::SignedLt(l, r) => {
            assert!(
                is_signed_ref(l, b) && is_signed_ref(r, a),
                "expected `>` to lower as SignedLt with its operands swapped"
            );
        }
        other => panic!("expected `$signed(a) > $signed(b)` as SignedLt(b, a), got {other:?}"),
    }
    // le: $signed(a) <= $signed(b)  ->  !(b < a)
    match find_assign(body, le) {
        Expr::Not(inner) => match &**inner {
            Expr::SignedLt(l, r) => {
                assert!(
                    is_signed_ref(l, b) && is_signed_ref(r, a),
                    "expected `<=` to lower as !(b < a)"
                );
            }
            other => panic!("expected SignedLt(b, a) inside the negation, got {other:?}"),
        },
        other => panic!("expected `$signed(a) <= $signed(b)` as Not(SignedLt(b, a)), got {other:?}"),
    }
    // ge: $signed(a) >= $signed(b)  ->  !(a < b)
    match find_assign(body, ge) {
        Expr::Not(inner) => match &**inner {
            Expr::SignedLt(l, r) => {
                assert!(
                    is_signed_ref(l, a) && is_signed_ref(r, b),
                    "expected `>=` to lower as !(a < b)"
                );
            }
            other => panic!("expected SignedLt(a, b) inside the negation, got {other:?}"),
        },
        other => panic!("expected `$signed(a) >= $signed(b)` as Not(SignedLt(a, b)), got {other:?}"),
    }
    // The unsigned control keeps using the ordinary unsigned comparison.
    match find_assign(body, unsigned_lt) {
        Expr::Lt(l, r) => {
            assert!(matches!(**l, Expr::Ref(id) if id == a));
            assert!(matches!(**r, Expr::Ref(id) if id == b));
        }
        other => panic!("expected `a < b` to stay an unsigned Expr::Lt, got {other:?}"),
    }
}

/// A comparison with exactly one `$signed(...)` operand is rejected:
/// Verilog says to compare those *unsigned*, which needs the signed
/// operand truncated back to its own width first (an 8-bit -1 has to read
/// as 255, not as the 64-bit sign-extended pattern), and that isn't
/// implemented.
#[test]
fn rejects_mixed_signed_unsigned_comparison() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/signed_compare_mixed_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "signed_compare_mixed_test.v compares a $signed value against an unsigned one, which v1 must reject",
    );
    assert!(
        err.contains("mixed signed/unsigned"),
        "expected the error to mention the mixed comparison, got: {err}"
    );
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
