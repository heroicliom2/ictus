use ictus_ir::Stmt;
use std::path::Path;

/// A call to a *provably-empty* task (`empty_statement;`, body
/// `begin end`, mirroring picorv32's own no-op-placeholder style for a
/// compiled-out `` `assert(...) ``) lowers to zero statements -- the
/// call itself vanishes, leaving only the real statement that follows it
/// in the same block.
#[test]
fn lowers_empty_task_call_as_a_no_op() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/task_call_test.v");
    let module =
        ictus_frontend_verilog::lower_file(&path).expect("task_call_test.v should lower cleanly");

    let count = module.signal_id("count").expect("count port");

    let Stmt::If { else_branch, .. } = &module.clocked_processes[0].body[0] else {
        panic!("expected the process body to start with the reset if/else");
    };
    assert_eq!(
        else_branch.len(),
        1,
        "the task call should contribute zero statements, leaving only `count <= count + 1`"
    );
    match &else_branch[0] {
        Stmt::NonBlockingAssign { target, .. } => assert_eq!(*target, count),
        other => panic!("expected `count <= count + 1`, got {other:?}"),
    }
}

/// A call to a task whose body is *not* provably empty must be rejected,
/// not silently treated as a no-op -- v1 doesn't model task execution at
/// all, so silently dropping a call to a task with real statements in it
/// would silently discard whatever behavior that task was meant to have.
#[test]
fn rejects_call_to_a_task_with_a_nonempty_body() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/task_call_nonempty_test.v");
    let err = ictus_frontend_verilog::lower_file(&path).expect_err(
        "task_call_nonempty_test.v calls a task with a real body, which v1 must reject",
    );
    assert!(
        err.contains("bump"),
        "expected the error to name the rejected task, got: {err}"
    );
}

/// A task call *with* arguments is rejected even when the task itself is
/// otherwise callable -- v1 has no notion of task ports to bind arguments
/// to.
#[test]
fn rejects_task_call_with_arguments() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/task_call_args_test.v");
    let err = ictus_frontend_verilog::lower_file(&path)
        .expect_err("task_call_args_test.v calls a task with an argument, which v1 must reject");
    assert!(
        err.contains("arguments"),
        "expected the error to mention arguments, got: {err}"
    );
}
