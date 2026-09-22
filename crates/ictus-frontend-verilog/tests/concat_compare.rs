use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// A comparison/logical operator result is always exactly 1 bit in
/// Verilog -- not a guess the way a general arithmetic result's width
/// would be -- so it's a valid concatenation operand, unlike `Add`/`Sub`/
/// `Mul`. picorv32 relies on exactly this to build combined decode/flag
/// signals from several comparisons.
#[test]
fn lowers_comparison_results_as_concatenation_operands() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concat_compare_test.v");
    let module = ictus_frontend_verilog::lower_file(&path)
        .expect("concat_compare_test.v should lower cleanly");

    let flags = module.signal_id("flags").expect("flags port");
    let Stmt::NonBlockingAssign { target, value, .. } = &module.clocked_processes[0].body[0]
    else {
        panic!("expected the process body to be a single non-blocking assignment");
    };
    assert_eq!(*target, flags);

    match value {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 4, "{{a==b, a<b, a>b, a!=b}} should have 4 parts");
            for (part, width) in parts {
                assert_eq!(*width, 1, "a comparison result is always 1 bit wide");
                assert!(
                    matches!(
                        part,
                        Expr::Eq(..) | Expr::Lt(..) | Expr::Gt(..) | Expr::Ne(..)
                    ),
                    "expected a comparison operator, got {part:?}"
                );
            }
        }
        other => panic!("expected `{{a==b, a<b, a>b, a!=b}}` as Expr::Concat, got {other:?}"),
    }
}
