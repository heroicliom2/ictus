use ictus_ir::{Expr, Module, SignalId, Stmt};
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn writes(stmts: &[Stmt], target: SignalId) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::NonBlockingAssign { target: t, .. } | Stmt::BlockingAssign { target: t, .. } => {
            *t == target
        }
        Stmt::If { then_branch, else_branch, .. } => {
            writes(then_branch, target) || writes(else_branch, target)
        }
        Stmt::Case { arms, default, .. } => {
            arms.iter().any(|a| writes(&a.body, target)) || writes(default, target)
        }
        _ => false,
    })
}

/// How many clocked processes, combinational processes and continuous
/// assignments drive `name`.
fn drivers(module: &Module, name: &str) -> (usize, usize, usize) {
    let id = module.signal_id(name).unwrap_or_else(|| panic!("signal {name}"));
    (
        module.clocked_processes.iter().filter(|p| writes(&p.body, id)).count(),
        module.comb_processes.iter().filter(|p| writes(&p.body, id)).count(),
        module.assigns.iter().filter(|a| a.target == id).count(),
    )
}

/// Only the branch a `generate if` selects exists in the lowered design.
///
/// Before elaboration, sv-parser's deep iterator yielded *both* branches
/// and the frontend lowered both: picorv32's `generate if (TWO_CYCLE_ALU)`
/// produced a clocked and a combinational ALU driving the same signals,
/// and the design worked only because the combinational one ran last.
/// Here the two parameters select opposite branches, so a correct result
/// has exactly one driver per signal, of the right kind. See
/// docs/decisions.md D27.
#[test]
fn lowers_only_the_selected_generate_branch() {
    let module = ictus_frontend_verilog::lower_file(&fixture("generate_test.v"))
        .expect("generate_test.v should lower cleanly");

    assert_eq!(drivers(&module, "sum"), (1, 0, 0), "REG_SUM = 1 selects the clocked branch");
    assert_eq!(drivers(&module, "diff"), (0, 1, 0), "REG_DIFF = 0 selects the combinational one");

    // MODE = 2 falls through both `else if` tests to the final `else`.
    assert_eq!(drivers(&module, "picked"), (0, 0, 1));
    let picked = module.signal_id("picked").unwrap();
    let assign = module.assigns.iter().find(|a| a.target == picked).unwrap();
    assert!(
        matches!(assign.value, Expr::Xor(..)),
        "expected the `a ^ b` branch, got {:?}",
        assign.value
    );
}

/// A module instantiation inside a branch that *isn't* selected is
/// neither lowered nor rejected -- it doesn't exist in the elaborated
/// design. This is what lets picorv32 lower at all: its `ENABLE_MUL` and
/// `ENABLE_DIV` instances sit in branches its defaults don't select.
#[test]
fn ignores_an_instance_in_an_unselected_branch() {
    let module =
        ictus_frontend_verilog::lower_file(&fixture("generate_unselected_instance_test.v"))
            .expect("an instance in an unselected branch should not prevent lowering");
    assert_eq!(drivers(&module, "y"), (0, 0, 1));
}

/// The same instance in the branch that *is* selected is real, so it is
/// lowered -- and here that fails, because `some_extra_unit` isn't
/// declared anywhere. The point is the contrast with the test above: the
/// identical instantiation is ignored in an unselected branch and must
/// resolve in a selected one. (Before instantiation was supported, this
/// was rejected as an instantiation; before *that*, it was dropped
/// silently, and picorv32 with `ENABLE_MUL=1` would have lowered with no
/// multiplier and no error.)
#[test]
fn resolves_an_instance_in_the_selected_branch() {
    let err = ictus_frontend_verilog::lower_file(&fixture("generate_selected_instance_test.v"))
        .expect_err("the selected branch instantiates a module that doesn't exist");
    assert!(
        err.contains("some_extra_unit") && err.contains("no ANSI-style module"),
        "expected the error to name the missing module, got: {err}"
    );
}

#[test]
fn rejects_generate_for() {
    let err = ictus_frontend_verilog::lower_file(&fixture("generate_for_test.v"))
        .expect_err("generate_for_test.v uses `generate for`, which v1 rejects");
    assert!(err.contains("generate for"), "got: {err}");
}

/// A `localparam` inside a `generate if` is rejected. Parameters are
/// resolved before branches are selected, so the common idiom of
/// declaring the same name in both branches would silently resolve to
/// whichever came last -- here, `SHIFT = 2` though `WIDE = 1` selects the
/// branch that says 1.
#[test]
fn rejects_a_parameter_declared_inside_a_generate_if() {
    let err = ictus_frontend_verilog::lower_file(&fixture("generate_param_test.v"))
        .expect_err("generate_param_test.v declares a localparam in a generate branch");
    assert!(err.contains("inside a `generate if`"), "got: {err}");
}

/// A signal driven from more than one process is rejected. This is the
/// exact shape picorv32's ALU lowered to before `generate if` was
/// elaborated -- a clocked and a combinational block both writing it --
/// and it simulated correctly only because the combinational one ran
/// last. With this check that state is refused outright rather than
/// depending on a test happening to sample between clock edges.
#[test]
fn rejects_a_signal_driven_from_two_processes() {
    let err = ictus_frontend_verilog::lower_file(&fixture("multi_driver_test.v"))
        .expect_err("multi_driver_test.v drives `y` from two blocks, which v1 must reject");
    assert!(
        err.contains("'y'") && err.contains("more than one place"),
        "expected the error to name the signal, got: {err}"
    );
}
