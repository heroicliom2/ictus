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

/// How many clocked and combinational processes write `name`.
fn process_drivers(module: &Module, name: &str) -> (usize, usize) {
    let id = module.signal_id(name).unwrap_or_else(|| panic!("signal {name}"));
    (
        module.clocked_processes.iter().filter(|p| writes(&p.body, id)).count(),
        module.comb_processes.iter().filter(|p| writes(&p.body, id)).count(),
    )
}

/// The value driven into `y`: `FLAG ? MAX : a`, with both parameters
/// already folded to literals.
fn y_value(module: &Module) -> &Expr {
    let y = module.signal_id("y").expect("y port");
    match &module.clocked_processes[0].body[0] {
        Stmt::NonBlockingAssign { target, value, .. } if *target == y => value,
        other => panic!("expected `y <= ...`, got {other:?}"),
    }
}

/// With no overrides, every parameter takes its declared default.
#[test]
fn defaults_apply_without_overrides() {
    let module = ictus_frontend_verilog::lower_file(&fixture("param_override_test.v"))
        .expect("param_override_test.v should lower");
    let a = module.signal_id("a").unwrap();
    assert_eq!(module.signals[a].width, 4);
    match y_value(&module) {
        Expr::Ternary { cond, then_val, .. } => {
            assert!(matches!(**cond, Expr::Literal { value: 0, .. }));
            assert!(
                matches!(**then_val, Expr::Literal { value: 15, .. }),
                "MAX = 2^4 - 1, got {then_val:?}"
            );
        }
        other => panic!("expected a ternary, got {other:?}"),
    }
}

/// An override replaces the default *before* anything later is resolved,
/// so what's derived from it follows: the port widths `[W-1:0]` and the
/// localparam `MAX = (1 << W) - 1` both see W = 8. See decisions.md D29.
#[test]
fn an_override_reaches_everything_derived_from_it() {
    let module = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("param_override_test.v"),
        &[("W", 8), ("FLAG", 1)],
    )
    .expect("param_override_test.v should lower with overrides");

    let a = module.signal_id("a").unwrap();
    let y = module.signal_id("y").unwrap();
    assert_eq!(module.signals[a].width, 8, "port width follows the override");
    assert_eq!(module.signals[y].width, 8);
    match y_value(&module) {
        Expr::Ternary { cond, then_val, .. } => {
            assert!(matches!(**cond, Expr::Literal { value: 1, .. }), "FLAG overridden");
            assert!(
                matches!(**then_val, Expr::Literal { value: 255, .. }),
                "MAX recomputed from the overridden W, got {then_val:?}"
            );
        }
        other => panic!("expected a ternary, got {other:?}"),
    }
}

/// An override that flips a `generate if` selects the other branch -- the
/// reason this feature exists. By default `generate_test.v` has `sum`
/// registered, `diff` combinational and `picked = a ^ b`; overriding all
/// three conditions swaps every one.
#[test]
fn an_override_selects_the_other_generate_branch() {
    let module = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("generate_test.v"),
        &[("REG_SUM", 0), ("REG_DIFF", 1), ("MODE", 0)],
    )
    .expect("generate_test.v should lower with overrides");

    assert_eq!(process_drivers(&module, "sum"), (0, 1), "sum is now combinational");
    assert_eq!(process_drivers(&module, "diff"), (1, 0), "diff is now registered");

    let picked = module.signal_id("picked").unwrap();
    let assign = module.assigns.iter().find(|a| a.target == picked).unwrap();
    assert!(
        matches!(assign.value, Expr::Ref(_)),
        "MODE = 0 selects `assign picked = a`, got {:?}",
        assign.value
    );
}

/// A name that isn't a parameter is rejected: a typo would otherwise run
/// the default configuration and look exactly like success.
#[test]
fn rejects_an_unknown_parameter() {
    let err = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("param_override_test.v"),
        &[("WIDHT", 8)],
    )
    .expect_err("WIDHT is not a parameter");
    assert!(err.contains("WIDHT"), "expected the error to name it, got: {err}");
}

/// A localparam cannot be overridden -- Verilog doesn't allow it.
#[test]
fn rejects_overriding_a_localparam() {
    let err = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("param_override_test.v"),
        &[("MAX", 3)],
    )
    .expect_err("MAX is a localparam");
    assert!(err.contains("localparam"), "got: {err}");
}

/// A value that doesn't fit the declared width is rejected. Icarus
/// silently truncates (`FLAG=2` on a `[0:0]` parameter would give 0),
/// which is exactly the surprise a configuration flag shouldn't have.
#[test]
fn rejects_a_value_wider_than_the_parameter() {
    let err = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("param_override_test.v"),
        &[("FLAG", 2)],
    )
    .expect_err("2 doesn't fit a 1-bit parameter");
    assert!(err.contains("does not fit"), "got: {err}");
}

#[test]
fn rejects_the_same_parameter_twice() {
    let err = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("param_override_test.v"),
        &[("W", 8), ("W", 6)],
    )
    .expect_err("W given twice is ambiguous");
    assert!(err.contains("more than once"), "got: {err}");
}

/// On the real design: `TWO_CYCLE_ALU = 1` makes picorv32's ALU the
/// clocked branch of its `generate if`, so `alu_add_sub` is written by a
/// clocked process and no combinational one -- the reverse of the
/// default. This is the branch decisions.md D27 found being lowered
/// alongside the other; it is now present only when asked for.
#[test]
fn picorv32_two_cycle_alu_selects_the_clocked_alu() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/designs/picorv32/picorv32.v");

    let default = ictus_frontend_verilog::lower_file(&path).expect("picorv32 should lower");
    assert_eq!(process_drivers(&default, "alu_add_sub"), (0, 1));

    let two_cycle =
        ictus_frontend_verilog::lower_file_with_parameters(&path, &[("TWO_CYCLE_ALU", 1)])
            .expect("picorv32 should lower with TWO_CYCLE_ALU = 1");
    assert_eq!(process_drivers(&two_cycle, "alu_add_sub"), (1, 0));
}
