use ictus_ir::{Direction, Expr, Stmt};
use std::path::Path;

#[test]
fn lowers_counter_module_correctly() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/counter.v");
    let module = ictus_frontend_verilog::lower_file(&path).expect("counter.v should lower cleanly");

    assert_eq!(module.name, "counter");

    let clk = module.signal_id("clk").expect("clk port");
    let resetn = module.signal_id("resetn").expect("resetn port");
    let count = module.signal_id("count").expect("count port");

    assert_eq!(module.signals[clk].direction, Some(Direction::Input));
    assert_eq!(module.signals[clk].width, 1);
    assert_eq!(module.signals[resetn].direction, Some(Direction::Input));
    assert_eq!(module.signals[resetn].width, 1);
    assert_eq!(module.signals[count].direction, Some(Direction::Output));
    assert_eq!(module.signals[count].width, 8);

    assert_eq!(module.clocked_processes.len(), 1);
    let process = &module.clocked_processes[0];
    assert_eq!(process.clock, clk);
    assert_eq!(process.body.len(), 1);

    let Stmt::If {
        cond,
        then_branch,
        else_branch,
    } = &process.body[0]
    else {
        panic!("expected the process body to be a single if/else statement");
    };

    // if (!resetn)
    match cond {
        Expr::Not(inner) => assert!(matches!(**inner, Expr::Ref(id) if id == resetn)),
        other => panic!("expected `!resetn`, got {other:?}"),
    }

    // then: count <= 8'd0;
    assert_eq!(then_branch.len(), 1);
    match &then_branch[0] {
        Stmt::NonBlockingAssign { target, value } => {
            assert_eq!(*target, count);
            assert!(matches!(value, Expr::Literal { value: 0, width: 8 }));
        }
        other => panic!("expected a non-blocking assignment, got {other:?}"),
    }

    // else: count <= count + 8'd1;
    assert_eq!(else_branch.len(), 1);
    match &else_branch[0] {
        Stmt::NonBlockingAssign { target, value } => {
            assert_eq!(*target, count);
            match value {
                Expr::Add(lhs, rhs) => {
                    assert!(matches!(**lhs, Expr::Ref(id) if id == count));
                    assert!(matches!(**rhs, Expr::Literal { value: 1, width: 8 }));
                }
                other => panic!("expected `count + 8'd1`, got {other:?}"),
            }
        }
        other => panic!("expected a non-blocking assignment, got {other:?}"),
    }
}
