use ictus_ir::{Expr, Stmt};
use std::path::Path;

/// `{N{expr}}` (a single-expression replication, mirroring picorv32's
/// own `mem_la_write & {4{...}}` style) and `{N{a, b}}` (replicating a
/// *multi*-part inner concatenation) both lower to a flat `Expr::Concat`
/// whose part list is the inner concatenation's own parts physically
/// repeated `N` times -- no new IR needed.
#[test]
fn lowers_replication_concatenation() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replicate_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("replicate_test.v should lower cleanly");

    let flag = module.signal_id("flag").expect("flag port");
    let a = module.signal_id("a").expect("a port");
    let b = module.signal_id("b").expect("b port");
    let masked = module.signal_id("masked").expect("masked port");
    let doubled = module.signal_id("doubled").expect("doubled port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };

    // masked <= 8'hFF & {8{flag}};  -- {8{flag}} is 8 copies of `flag`.
    match find_assign(else_branch, masked) {
        Expr::And(_, rhs) => match &**rhs {
            Expr::Concat(parts) => {
                assert_eq!(parts.len(), 8, "{{8{{flag}}}} should replicate to 8 parts");
                for (part, width) in parts {
                    assert!(matches!(part, Expr::Ref(id) if *id == flag));
                    assert_eq!(*width, 1);
                }
            }
            other => panic!("expected `{{8{{flag}}}}` as the And's rhs, got {other:?}"),
        },
        other => panic!("expected `8'hFF & {{8{{flag}}}}`, got {other:?}"),
    }

    // doubled <= {2{a, b}};  -- {a,b} repeated twice: a,b,a,b.
    match find_assign(else_branch, doubled) {
        Expr::Concat(parts) => {
            assert_eq!(parts.len(), 4, "{{2{{a, b}}}} should replicate to 4 parts (a,b,a,b)");
            assert!(matches!(&parts[0], (Expr::Ref(id), 2) if *id == a));
            assert!(matches!(&parts[1], (Expr::Ref(id), 2) if *id == b));
            assert!(matches!(&parts[2], (Expr::Ref(id), 2) if *id == a));
            assert!(matches!(&parts[3], (Expr::Ref(id), 2) if *id == b));
        }
        other => panic!("expected `{{2{{a, b}}}}` as Expr::Concat with 4 parts, got {other:?}"),
    }
}

/// A non-constant replication count (`{n{flag}}`, `n` a signal) must be
/// rejected -- the count has to be known at lowering time to build a
/// fixed-size `Expr::Concat`, the same restriction a bit-select/
/// part-select bound already has.
#[test]
fn rejects_variable_replication_count() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/replicate_variable_count_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "replicate_variable_count_test.v uses a signal as the replication count, which v1 must reject",
    );
    assert!(
        err.contains("compile-time constant"),
        "expected the error to mention a compile-time constant, got: {err}"
    );
}

/// A replication count of `0` (`{0{flag}}`, legal Verilog for a
/// deliberate zero-width contribution) is rejected in v1 -- `Expr::Concat`
/// can't represent an empty part list, same restriction a plain `{}`
/// already has.
#[test]
fn rejects_zero_replication_count() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replicate_zero_count_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "replicate_zero_count_test.v uses a replication count of 0, which v1 must reject",
    );
    assert!(
        err.contains('0'),
        "expected the error to mention the zero count, got: {err}"
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
