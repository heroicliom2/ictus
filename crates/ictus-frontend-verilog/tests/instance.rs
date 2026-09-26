use ictus_ir::{Expr, Module};
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn lower_err(name: &str) -> String {
    ictus_frontend_verilog::lower_file(&fixture(name))
        .expect_err("this fixture must be rejected")
}

fn assign_to<'m>(module: &'m Module, name: &str) -> &'m Expr {
    let id = module.signal_id(name).unwrap_or_else(|| panic!("signal {name}"));
    &module
        .assigns
        .iter()
        .find(|a| a.target == id)
        .unwrap_or_else(|| panic!("nothing assigns {name}"))
        .value
}

/// Instances are flattened into their parent: every signal of an instance
/// becomes a parent signal named `<instance>.<name>`, except a port wired
/// straight to a whole parent signal of the same width, which is *aliased*
/// -- the child's references point at the parent's signal, and no copy
/// exists. See docs/decisions.md D30.
#[test]
fn flattens_instances_into_the_parent() {
    let module = ictus_frontend_verilog::lower_file(&fixture("instance_test.v"))
        .expect("instance_test.v should lower");

    // Only the top's ports remain ports.
    let ports: Vec<&str> = module
        .signals
        .iter()
        .filter(|s| s.direction.is_some())
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        ports,
        vec!["clk", "resetn", "a", "b", "sum_q", "diff_q", "narrow_q", "count_a", "last_sum"]
    );

    // Each instance has its own copy of the internal register -- the two
    // `accum`s' `prev`s must not collide.
    for prev in ["u_sum.prev", "u_diff.prev", "u_narrow.prev"] {
        assert!(module.signal_id(prev).is_some(), "{prev} should exist");
    }

    // Aliased ports leave no copy behind: `.q(sum_q)`, `.clk(clk)`, and the
    // grandchild's `.value(count)` -> `.count(count_a)`, two levels deep.
    for aliased in ["u_sum.q", "u_sum.clk", "u_sum.count", "u_sum.u_count.value"] {
        assert!(module.signal_id(aliased).is_none(), "{aliased} should be aliased away");
    }

    // An input connected to an expression is a copy, driven by it.
    assert!(matches!(assign_to(&module, "u_sum.in"), Expr::Add(..)));
    assert!(matches!(assign_to(&module, "u_diff.in"), Expr::Sub(..)));

    // An 8-bit output into a 4-bit signal goes through a copy of the port
    // and an assignment, which the kernel truncates on write.
    let narrow_copy = module.signal_id("u_narrow.q").expect("u_narrow.q copied");
    assert_eq!(module.signals[narrow_copy].width, 8);
    assert!(matches!(assign_to(&module, "narrow_q"), Expr::Ref(id) if *id == narrow_copy));

    // Three `accum`s, each with its own block and its `counter`'s: six
    // clocked processes, every one on the top's `clk` because every `.clk`
    // connection was aliased.
    let clk = module.signal_id("clk").unwrap();
    assert_eq!(module.clocked_processes.len(), 6);
    assert!(module.clocked_processes.iter().all(|p| p.clock == clk));
}

/// Parameters pass down: `W` in the top becomes `WIDTH` in each `accum`,
/// and `WIDTH` in each `counter`. Overriding `W` at the top reaches all of
/// them, through two levels of instantiation.
#[test]
fn parameters_pass_down_through_the_hierarchy() {
    let module = ictus_frontend_verilog::lower_file_with_parameters(
        &fixture("instance_test.v"),
        &[("W", 16)],
    )
    .expect("instance_test.v should lower with W = 16");

    for name in ["a", "u_sum.in", "u_sum.prev", "u_narrow.q", "u_diff.count"] {
        let id = module.signal_id(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(module.signals[id].width, 16, "{name}");
    }
}

#[test]
fn rejects_positional_port_connections() {
    let err = lower_err("inst_positional_test.v");
    assert!(err.contains("by position"), "got: {err}");
}

/// An input left unconnected would float at `z` in Verilog, which a
/// 2-state kernel can't represent -- and it is usually a mistake.
#[test]
fn rejects_an_unconnected_input() {
    let err = lower_err("inst_unconnected_input_test.v");
    assert!(err.contains("'in'") && err.contains("not connected"), "got: {err}");
}

/// An output wired to part of a signal would need a continuous assignment
/// to a part-select, which v1 doesn't support.
#[test]
fn rejects_an_output_connected_to_a_part_select() {
    let err = lower_err("inst_output_select_test.v");
    assert!(err.contains("output port 'out'"), "got: {err}");
}

#[test]
fn rejects_a_module_that_instantiates_itself() {
    let err = lower_err("inst_recursive_test.v");
    assert!(err.contains("instantiates itself"), "got: {err}");
}

#[test]
fn rejects_an_unknown_port() {
    let err = lower_err("inst_unknown_port_test.v");
    assert!(err.contains("no port named 'outt'"), "got: {err}");
}

/// A child clocked by a gated version of the parent's clock would be
/// ticked together with everything else and be silently wrong. The kernel
/// has always assumed one clock; instances are what make breaking that
/// assumption easy, so it is now checked.
#[test]
fn rejects_a_second_clock_domain() {
    let err = lower_err("inst_gated_clock_test.v");
    assert!(err.contains("more than one clock"), "got: {err}");
}

/// A child's output wired onto the parent's own input port would be
/// aliased onto it and silently fight whatever drives the input.
#[test]
fn rejects_driving_an_input_port_from_inside() {
    let err = lower_err("inst_drives_input_test.v");
    assert!(err.contains("input port 'a'"), "got: {err}");
}
