use ictus_ir::{Direction, Expr, Stmt};
use std::path::Path;

/// Covers three fixes made together while chasing real errors from
/// lowering the actual picorv32 benchmark design (bench/designs/picorv32):
/// a port that inherits its direction from the previous one in the list
/// (`input clk, resetn,` -- picorv32's own first two ports, verbatim
/// style), a single declaration naming several signals
/// (`reg [7:0] a, b, c;` -- picorv32 does this too), and the ternary
/// operator (used throughout picorv32, not supported at all before this).
#[test]
fn lowers_inherited_direction_multi_name_decl_and_ternary() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/decl_style_test.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("decl_style_test.v should lower cleanly");

    // Port direction inheritance: `input clk, resetn,` -- both must come
    // out as Direction::Input, even though only `clk` carries an explicit
    // `input` keyword in the source.
    let clk = module.signal_id("clk").expect("clk port");
    let resetn = module.signal_id("resetn").expect("resetn port");
    assert_eq!(module.signals[clk].direction, Some(Direction::Input));
    assert_eq!(module.signals[resetn].direction, Some(Direction::Input));

    // Multi-name declaration: `reg [7:0] a, b, c;` -- three distinct
    // internal signals, all 8 bits wide.
    let a = module.signal_id("a").expect("signal a");
    let b = module.signal_id("b").expect("signal b");
    let c = module.signal_id("c").expect("signal c");
    for id in [a, b, c] {
        assert_eq!(module.signals[id].direction, None, "a/b/c are internal, not ports");
        assert_eq!(module.signals[id].width, 8);
    }
    assert_ne!(a, b);
    assert_ne!(b, c);
    assert_ne!(a, c);

    // Nested ternary: `(a > b) ? (a > c ? a : c) : (b > c ? b : c)`.
    let result = module.signal_id("result").expect("result port");
    let Stmt::NonBlockingAssign {
        target,
        value: outer,
    } = &module.clocked_processes[0].body[1]
    else {
        panic!("expected the second statement to be `result <= ...`");
    };
    assert_eq!(*target, result);

    let Expr::Ternary {
        cond,
        then_val,
        else_val,
    } = outer
    else {
        panic!("expected a top-level ternary, got {outer:?}");
    };
    assert!(matches!(**cond, Expr::Gt(ref l, ref r) if matches!(**l, Expr::Ref(id) if id == a) && matches!(**r, Expr::Ref(id) if id == b)));

    let Expr::Ternary { cond: inner_cond, .. } = &**then_val else {
        panic!("expected the then-branch to itself be a ternary (`a > c ? a : c`), got {then_val:?}");
    };
    assert!(matches!(**inner_cond, Expr::Gt(ref l, ref r) if matches!(**l, Expr::Ref(id) if id == a) && matches!(**r, Expr::Ref(id) if id == c)));

    let Expr::Ternary { cond: inner_cond, .. } = &**else_val else {
        panic!("expected the else-branch to itself be a ternary (`b > c ? b : c`), got {else_val:?}");
    };
    assert!(matches!(**inner_cond, Expr::Gt(ref l, ref r) if matches!(**l, Expr::Ref(id) if id == b) && matches!(**r, Expr::Ref(id) if id == c)));
}
