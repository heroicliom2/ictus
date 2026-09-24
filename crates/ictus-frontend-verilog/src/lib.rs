//! Verilog frontend: parses source via `sv-parser` and lowers the result
//! into `ictus_ir`.
//!
//! v1 scope, matching `ictus_ir`'s current shape (see that crate's doc
//! comment): a single ANSI-style module (`module foo (input wire clk,
//! ...)`, ports may inherit direction from the previous one in the list
//! -- `input clk, resetn,` -- see `lower_port`), any number of clocked
//! `always @(posedge clk) begin ... end` blocks, `if`/`else`/`else if`
//! chains (lowered to nested `Stmt::If` -- see `lower_if` -- no new IR
//! needed), `case`/`casez`/`casex` (wildcard bits only on a case *item*'s
//! own literal -- see `lower_case`/`lower_case_value`), both non-blocking
//! (`<=`) and blocking (`=`) assignment -- which differ only in *when*
//! the write lands, and so share one `lower_procedural_assign` and differ
//! only in which `Stmt` it emits; see decisions.md D23 --
//! internal `wire`/`reg` declarations naming one or more
//! signals per declaration (`reg a, b, c;` -- see `lower_internal_signal`)
//! in addition to ports, array/memory signals (`reg [7:0] mem [0:3];` --
//! one unpacked `[high:low]` dimension, internal signals only, read and
//! written one element at a time at a runtime index; see
//! `lower_array_index` and `ictus_ir::Signal::depth`), `#(parameter
//! [7:0] X = 1)` *and* `localparam`
//! (sharing one resolution pass and name table -- see `lower_parameters`;
//! neither is a signal, and neither ever appears in `ictus_ir::Module` at
//! all, since every reference is resolved to a plain `Expr::Literal`
//! directly at lowering time), a default/value expression built from
//! Verilog's separate *constant*-expression grammar (`lower_constant_expr`/
//! `lower_constant_primary` -- literals, an earlier parameter/localparam
//! reference, `+ - *`, the ternary operator, and concatenation; see
//! `try_const_fold`, reused everywhere else in this file a compile-time
//! constant is needed, for why this and the general expression grammar
//! ultimately build the same `ictus_ir::Expr` shape), constant and
//! variable bit-select, constant part-select, concatenation
//! (plain `{a,b}` and replication/multiple concatenation `{N{a,b}}` --
//! see `lower_multiple_concatenation`, which needs no new IR: the count
//! must fold to a compile-time constant, same restriction a bit-select/
//! part-select bound already has, and the result is just the inner
//! concatenation's own parts physically repeated `N` times), and the
//! ternary operator on the *read* side (`x[3]`, `x[i]`, `x[7:0]`, `{a,b}`,
//! `{4{a,b}}`, `c ? a : b`; no indexed part-select `x[base +: width]`), plus
//! on the *assignment-target* side: a *constant* bit-select/part-select as
//! a procedural-assignment target (`x[7:0] <= v;`, picorv32's
//! `mem_rdata_q[...] <= ...` style -- see `lower_select_target_range`),
//! and a concatenation of such targets (`{a, b[3:0]} <= v;`, picorv32's
//! `{mem_rdata_q[31:25], mem_rdata_q[11:7]} <= ...` style -- see
//! `lower_concat_target_assign`, which splits it into one plain
//! assignment statement per part rather than needing new IR; a
//! *nested* concatenation inside the target is rejected, not guessed at).
//! A variable index/indexed-range target, and any select or concatenation
//! as a *continuous*-assignment target, are still rejected: a procedural
//! assignment reaches a shared write path that does the read-modify-write
//! (`ictus_kernel`'s `apply_write`), and a continuous one has no
//! equivalent -- see the
//! `NetLvalue::Lvalue` check in `lower_continuous_assign`. And expressions
//! built from literals (decimal/binary/hex, not octal; a 4-state `x`/`z`
//! digit -- whole-value or mixed with real digits, any base, *outside* a
//! `case`/`casez`/`casex` item's own wildcard matching -- resolves to the
//! bit `0`, see `parse_binary_literal_value`/`parse_hex_literal_value`
//! and decisions.md D19), signal references, unary logical `!`, bitwise
//! complement `~`, and the reduction operators `& | ^ ~& ~| ~^`/`^~` (see
//! `lower_expr`'s `E::Unary` arm and `ictus_ir::Expr::BitwiseNot`/
//! `ReduceAnd`/`ReduceOr`/`ReduceXor`'s doc comments -- NAND/NOR/XNOR
//! compose `BitwiseNot` with a reduction at lowering time rather than
//! getting their own IR), the binary operators
//! `+ - * << >> >>> & | ^ == != < <= > >= && ||` -- several of which key
//! off whether their operands are `$signed(...)`: `>>>` lowers to a real
//! arithmetic shift only for a `$signed(...)` left operand (the only case
//! Verilog makes it differ from `>>`), `>>` of a `$signed(...)` value is
//! rejected rather than shifting that value's sign-extension bits down,
//! and an ordering comparison lowers to a real *signed* comparison when
//! *both* operands are `$signed(...)` (rejecting the mixed case, which
//! Verilog compares unsigned via a truncation this doesn't implement) --
//! see `apply_binary_op`. And the `$signed(...)`
//! system function (see `lower_system_function_call` and
//! `ictus_ir::Expr::Signed`'s doc comment -- it sign-extends a value into
//! a wider assignment target, and marks operands for the signed-aware
//! operators listed above; it is *not* a general signed type system, so
//! anything beyond those is rejected rather than guessed at). Also lowers a
//! statement-level call to a *provably-empty* task (`some_task;`, no
//! arguments -- see `lower_task_call_statement` and
//! `lower_task_declarations`) as a true no-op, since v1 doesn't model
//! task execution at all; picorv32 relies on exactly this for its own
//! `` `assert(...) `` macro, which expands to a call to a deliberately-empty
//! task when assertions are compiled out. A call to any other task (a
//! real body, arguments, a system task, a task not declared in this
//! module) is rejected, not silently treated as a no-op. `lower_expr`'s
//! `E::Binary` arm also corrects a real `sv-parser` precedence-handling
//! gap: it
//! mis-associates an *unparenthesized* ternary immediately following a
//! binary operator's right operand (`a > c ? a : c` structured as if it
//! were `a > (c ? a : c)`, when real Verilog precedence makes `(a > c) ?
//! a : c` the only correct reading) -- confirmed empirically, not
//! assumed, against both forms; see that arm's own comment and
//! decisions.md D13 for the full story. Anything else in the source is
//! either ignored (other module items) or produces an error, deliberately
//! -- silently mis-lowering an unsupported construct would make this
//! project's own differential testing (docs/architecture.md, Validation
//! strategy) meaningless. Also lowers single-assignment continuous
//! `assign target = expr;` statements (net-targeted only -- see
//! `lower_continuous_assign`) into `ictus_ir::Assign`. Widen this as
//! later phases need more of the language. The diagnostic's next gap is
//! narrower than the last few: `expr_width` can't determine the width of
//! a bitwise `&`/`|`/`^` result, so one can't yet be a *concatenation*
//! operand (see `expr_width`, and roadmap.md for why extending it to the
//! arithmetic operators at the same time is a real decision rather than
//! the same change twice). Beyond that, compound assignment (`+=`),
//! `always_comb`, and module instantiation are the
//! next-highest-value gaps toward running a real design like phase 0's
//! picorv32 benchmark.

use ictus_ir::{
    Assign, CaseArm, CaseValue, ClockedProcess, Direction, Expr, Module, Signal, SignalId, Stmt,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use sv_parser::{
    parse_sv, unwrap_node, AlwaysConstruct, AnsiPortDeclaration, ConditionalStatement,
    ContinuousAssignNet, DataDeclaration, DecimalNumber, EdgeIdentifier, IntegralNumber, Locate,
    NonblockingAssignment, Number, PortDirection, RefNode, SeqBlock, StatementItem,
    StatementOrNull, SyntaxTree,
};

/// Bundles the module being lowered with its resolved parameter table and
/// its set of provably-empty task names, threaded through statement/
/// expression lowering so `lower_primary`/`lower_task_call_statement` can
/// resolve an identifier against them. Derefs to `Module` so every
/// existing `module.signal_id(...)`/`module.signals[...]` call site
/// throughout this file keeps working unchanged -- only those two
/// functions need the extra fields directly. Neither parameters nor
/// tasks ever appear in the final `ictus_ir::Module`: a parameter
/// reference is fully resolved to an `Expr::Literal` during lowering (see
/// `lower_parameters`), and a call to an empty task lowers to no
/// statements at all (see `lower_task_declarations`), so the IR and the
/// kernel never need to know either existed.
struct Ctx<'a> {
    module: &'a Module,
    parameters: &'a HashMap<String, (u64, u32)>,
    empty_tasks: &'a HashSet<String>,
}

impl<'a> std::ops::Deref for Ctx<'a> {
    type Target = Module;
    fn deref(&self) -> &Module {
        self.module
    }
}

pub fn lower_file(path: &Path) -> Result<Module, String> {
    let defines = std::collections::HashMap::new();
    let includes: Vec<std::path::PathBuf> = vec![];
    let (tree, _) = parse_sv(path, &defines, &includes, false, false)
        .map_err(|e| format!("parse error: {e}"))?;

    let module_node = tree
        .into_iter()
        .find_map(|n| match n {
            RefNode::ModuleDeclarationAnsi(x) => Some(x),
            _ => None,
        })
        .ok_or("no ANSI-style module declaration found")?;

    let name_node =
        unwrap_node!(module_node, ModuleIdentifier).ok_or("module has no identifier")?;
    let name_simple =
        unwrap_node!(name_node, SimpleIdentifier).ok_or("module identifier unreadable")?;
    let name = ident_str(name_simple, &tree)
        .ok_or("could not read module identifier")?
        .to_string();

    let mut module = Module {
        name,
        ..Default::default()
    };

    // Resolved before anything else, matching source order (`module foo
    // #(parameters) (ports)`) and so that a parameter's default value can
    // reference an *earlier* parameter, per real Verilog elaboration
    // order -- see lower_parameters.
    let parameters = lower_parameters(module_node, &tree)?;

    // Scanned up front, same reasoning as parameters: a task-call
    // statement (see lower_task_call_statement) needs to know whether the
    // task it names has a provably-empty body before it can decide
    // whether to accept the call as a no-op.
    let empty_tasks = lower_task_declarations(module_node, &tree)?;

    // A port that omits its own `input`/`output` keyword (`input clk,
    // resetn,` -- resetn has no keyword of its own) inherits the
    // direction of the previous port in the same list, per IEEE 1800 --
    // real, common style (picorv32 uses it in its very first two ports),
    // not an edge case. last_direction tracks that across the loop.
    let mut last_direction: Option<Direction> = None;
    for port_node in module_node.into_iter() {
        if let RefNode::AnsiPortDeclaration(port) = port_node {
            module.push_signal(lower_port(port, &tree, &mut last_direction, &parameters)?);
        }
    }

    for decl_node in module_node.into_iter() {
        match decl_node {
            RefNode::NetDeclaration(sv_parser::NetDeclaration::NetType(net)) => {
                for signal in lower_internal_signal(&**net, &tree, &parameters)? {
                    module.push_signal(signal);
                }
            }
            RefNode::DataDeclaration(DataDeclaration::Variable(var)) => {
                for signal in lower_internal_signal(&**var, &tree, &parameters)? {
                    module.push_signal(signal);
                }
            }
            _ => {}
        }
    }

    // Every remaining item (always blocks, assigns) may reference a
    // signal *or* a parameter, so all of them lower through `ctx` rather
    // than `&module` directly from here on. `ctx` borrows `module`
    // immutably for the rest of this scope, so its results are collected
    // into plain Vecs here and only written into `module` once that
    // borrow ends, rather than pushing into `module` while `ctx` is still
    // alive (which the borrow checker won't allow).
    let ctx = Ctx {
        module: &module,
        parameters: &parameters,
        empty_tasks: &empty_tasks,
    };

    let mut clocked_processes = Vec::new();
    for always_node in module_node.into_iter() {
        if let RefNode::AlwaysConstruct(always) = always_node {
            if let Some(process) = lower_always(always, &tree, &ctx)? {
                clocked_processes.push(process);
            }
        }
    }

    let mut assigns = Vec::new();
    for assign_node in module_node.into_iter() {
        if let RefNode::ContinuousAssign(sv_parser::ContinuousAssign::Net(net)) = assign_node {
            assigns.push(lower_continuous_assign(net, &tree, &ctx)?);
        }
    }

    module.clocked_processes = clocked_processes;
    module.assigns = assigns;

    Ok(module)
}

/// Parses every `#(parameter ...)` *and* `localparam ...` declaration in
/// the module into one shared name -> (value, width) table -- v1 doesn't
/// support instantiation at all, so the one real difference between the
/// two (a `parameter` can be overridden at instantiation, a `localparam`
/// never can) doesn't matter yet; both are simply named compile-time
/// constants. Neither is ever simulated as a signal -- every reference is
/// fully resolved to an `Expr::Literal` right here at lowering time,
/// substituted directly into the expression tree (see `lower_primary`),
/// so `ictus_ir::Module` and the kernel never need to know either exists.
/// Walking the whole module in one pass, in source order, and growing the
/// same table as it goes (rather than resolving parameters and
/// localparams in two separate passes) is what makes cross-references
/// work: a `localparam` can reference an earlier `parameter` (picorv32's
/// `regindex_bits` references three of its own module parameters, e.g.
/// `localparam integer regindex_bits = (ENABLE_REGS_16_31 ? 5 : 4) +
/// ENABLE_IRQ*ENABLE_IRQ_QREGS;`) simply because, by source order, the
/// `#(parameter ...)` port list is always visited before the module
/// body's own `localparam`s are. This is a pragmatic "declaration
/// precedes use" assumption, not a real dependency solve -- correct for
/// every real case found so far, not guaranteed by the Verilog grammar
/// itself in general.
fn lower_parameters(
    module_node: &sv_parser::ModuleDeclarationAnsi,
    tree: &SyntaxTree,
) -> Result<HashMap<String, (u64, u32)>, String> {
    let mut parameters = HashMap::new();

    for node in module_node.into_iter() {
        match node {
            RefNode::ParameterDeclarationParam(param_decl) => {
                resolve_param_assignments(
                    &param_decl.nodes.1,
                    &param_decl.nodes.2,
                    tree,
                    &mut parameters,
                )?;
            }
            RefNode::LocalParameterDeclarationParam(local_decl) => {
                resolve_param_assignments(
                    &local_decl.nodes.1,
                    &local_decl.nodes.2,
                    tree,
                    &mut parameters,
                )?;
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Shared body for one `#(parameter ...)` or `localparam ...` declaration
/// -- `ParameterDeclarationParam` and `LocalParameterDeclarationParam`
/// have identical `(Keyword, DataTypeOrImplicit, ListOfParamAssignments)`
/// shapes (different Rust types, since sv-parser generates a distinct
/// struct per grammar production, but the same fields), so this takes
/// just the two fields that actually matter rather than being duplicated
/// per keyword.
fn resolve_param_assignments(
    data_type: &sv_parser::DataTypeOrImplicit,
    assignments: &sv_parser::ListOfParamAssignments,
    tree: &SyntaxTree,
    parameters: &mut HashMap<String, (u64, u32)>,
) -> Result<(), String> {
    let width = match unwrap_node!(data_type, PackedDimensionRange) {
        Some(range_node) => lower_packed_range(range_node, tree, parameters)?,
        // No explicit range (`localparam integer regindex_bits = ...;`,
        // or a bare `parameter X = ...;`) -- 32 bits either way: Verilog's
        // default `integer`/untyped-parameter width.
        None => 32,
    };

    for assignment in assignments.nodes.0.contents() {
        let ident = unwrap_node!(&assignment.nodes.0, SimpleIdentifier)
            .ok_or("parameter has no identifier")?;
        let name = ident_str(ident, tree)
            .ok_or("parameter identifier unreadable")?
            .to_string();

        let Some((_, default)) = &assignment.nodes.2 else {
            return Err(format!(
                "parameter '{name}' has no default value (overriding a parameter at \
                 instantiation is not supported in v1)"
            ));
        };
        let value = lower_constant_param_expression(default, tree, parameters)
            .map_err(|e| format!("parameter '{name}' default: {e}"))?;

        parameters.insert(name, (value, width));
    }

    Ok(())
}

/// Lowers a `parameter`/`localparam` default value -- Verilog's
/// `ConstantParamExpression -> ConstantMintypmaxExpression -> ...`
/// grammar, a genuinely separate parallel hierarchy from the general
/// `Expression`/`Primary` `lower_expr`/`lower_primary` consume (also used
/// for a packed-range bound, `[msb:lsb]` -- see `lower_packed_range`,
/// which shares this same walk). Builds an ordinary `ictus_ir::Expr` via
/// `lower_constant_expr` (reusing `apply_binary_op`, exactly like the
/// general grammar does) and then folds it down to a plain `u64` with
/// `try_const_fold` -- guaranteed to succeed, since `lower_constant_expr`
/// can only ever produce a tree built from literals and already-resolved
/// parameter references (never a signal), but going through the same
/// fold helper used elsewhere keeps there being exactly one definition of
/// "is this actually constant" in the whole crate.
fn lower_constant_param_expression(
    expr: &sv_parser::ConstantParamExpression,
    tree: &SyntaxTree,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<u64, String> {
    let sv_parser::ConstantParamExpression::ConstantMintypmaxExpression(mtm) = expr else {
        return Err("parameter/localparam default must be a constant expression in v1".to_string());
    };
    let expr = match &**mtm {
        sv_parser::ConstantMintypmaxExpression::Unary(ce) => lower_constant_expr(ce, tree, parameters)?,
        sv_parser::ConstantMintypmaxExpression::Ternary(_) => {
            return Err("min:typ:max parameter/localparam values are not supported in v1".to_string())
        }
    };
    try_const_fold(&expr)
        .ok_or_else(|| "does not reduce to a compile-time constant".to_string())
}

/// Lowers Verilog's *constant*-expression grammar (`ConstantExpression`)
/// -- required for parameter/localparam defaults and packed-range bounds,
/// where only compile-time constants are legal -- into the same
/// `ictus_ir::Expr` shape the general grammar (`lower_expr`) produces,
/// reusing `apply_binary_op` so there's one definition of what each
/// operator means, not two. Only the forms real designs have needed so
/// far are handled: a literal, a reference to an *earlier* parameter/
/// localparam (see `lower_constant_primary`), `+ - *`, and the ternary
/// operator. Anything else (system functions, `inside`, ...) is rejected,
/// not guessed at.
fn lower_constant_expr(
    expr: &sv_parser::ConstantExpression,
    tree: &SyntaxTree,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<Expr, String> {
    use sv_parser::ConstantExpression as CE;
    match expr {
        CE::ConstantPrimary(primary) => lower_constant_primary(primary, tree, parameters),
        CE::Binary(binary) => {
            let op_text =
                symbol_text(&binary.nodes.1, tree).ok_or("binary operator unreadable")?;
            let lhs = lower_constant_expr(&binary.nodes.0, tree, parameters)?;
            let rhs = lower_constant_expr(&binary.nodes.3, tree, parameters)?;
            apply_binary_op(op_text, lhs, rhs)
        }
        CE::Ternary(ternary) => {
            let cond = lower_constant_expr(&ternary.nodes.0, tree, parameters)?;
            let then_val = lower_constant_expr(&ternary.nodes.3, tree, parameters)?;
            let else_val = lower_constant_expr(&ternary.nodes.5, tree, parameters)?;
            Ok(Expr::Ternary {
                cond: Box::new(cond),
                then_val: Box::new(then_val),
                else_val: Box::new(else_val),
            })
        }
        other => Err(format!("constant expression form not supported in v1: {other:?}")),
    }
}

/// The `ConstantPrimary` half of `lower_constant_expr` -- a literal, or a
/// reference to an already-resolved parameter/localparam (looked up in
/// the `parameters` table being built up by `lower_parameters`, so only
/// an *earlier* declaration is visible -- see that function's doc
/// comment). A select on the parameter reference itself (`FOO[3:0]`,
/// legal Verilog syntax on a parameter but not used by any real case
/// found so far) is rejected, not silently ignored.
fn lookup_constant_parameter(
    name: &str,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<Expr, String> {
    let &(value, width) = parameters
        .get(name)
        .ok_or_else(|| format!("reference to unknown parameter '{name}' in a constant expression"))?;
    Ok(Expr::Literal { value, width })
}

fn lower_constant_primary(
    primary: &sv_parser::ConstantPrimary,
    tree: &SyntaxTree,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<Expr, String> {
    use sv_parser::ConstantPrimary as CP;
    match primary {
        CP::PrimaryLiteral(lit) => match &**lit {
            sv_parser::PrimaryLiteral::Number(number) => lower_number(number, tree),
            other => Err(format!(
                "literal form not supported in a constant expression in v1: {other:?}"
            )),
        },
        CP::PsParameter(p) => {
            let select = &p.nodes.1;
            let has_select = select.nodes.0.is_some()
                || !select.nodes.1.nodes.0.is_empty()
                || select.nodes.2.is_some();
            if has_select {
                return Err(
                    "indexing into a parameter reference is not supported in a constant \
                     expression in v1"
                        .to_string(),
                );
            }
            let ident = unwrap_node!(&p.nodes.0, SimpleIdentifier)
                .ok_or("parameter reference identifier unreadable")?;
            let name =
                ident_str(ident, tree).ok_or("parameter reference identifier unreadable")?;
            lookup_constant_parameter(name, parameters)
        }
        // sv-parser's constant-expression grammar can parse a *bare*
        // identifier with no parentheses at all as a "constant function
        // call" instead of `PsParameter` -- confirmed empirically:
        // picorv32's own `ENABLE_REGS_16_31` (17 characters, no arguments
        // anywhere near it) parses this way inside
        // `localparam integer irqregs_offset = ENABLE_REGS_16_31 ? 32 : 16;`.
        // v1 has no notion of a constant *function* at all (`function` is
        // never lowered -- confirmed picorv32.v doesn't define one), so a
        // zero-argument call here is really just a parameter reference the
        // parser happened to classify differently, not a real function
        // call; one *with* arguments would be a genuine constant function
        // call this frontend can't evaluate, and is rejected.
        CP::ConstantFunctionCall(call) => {
            let sv_parser::SubroutineCall::TfCall(tf_call) = &call.nodes.0.nodes.0 else {
                return Err(
                    "this constant function/subroutine call form is not supported in v1"
                        .to_string(),
                );
            };
            if tf_call.nodes.2.is_some() {
                return Err(
                    "a constant function call with arguments is not supported in v1 (no \
                     `function` is lowered at all in v1, so this can only be a parameter \
                     reference sv-parser classified as a function call)"
                        .to_string(),
                );
            }
            let ident = unwrap_node!(&tf_call.nodes.0, SimpleIdentifier)
                .ok_or("parameter reference identifier unreadable")?;
            let name =
                ident_str(ident, tree).ok_or("parameter reference identifier unreadable")?;
            lookup_constant_parameter(name, parameters)
        }
        CP::MintypmaxExpression(paren) => match &paren.nodes.0.nodes.1 {
            sv_parser::ConstantMintypmaxExpression::Unary(ce) => {
                lower_constant_expr(ce, tree, parameters)
            }
            sv_parser::ConstantMintypmaxExpression::Ternary(_) => {
                Err("min:typ:max expressions are not supported in v1".to_string())
            }
        },
        CP::Concatenation(concat) => {
            if concat.nodes.1.is_some() {
                return Err(
                    "indexing into a concatenation is not supported in a constant expression \
                     in v1"
                        .to_string(),
                );
            }
            let exprs = concat.nodes.0.nodes.0.nodes.1.contents();
            if exprs.is_empty() {
                return Err("an empty concatenation `{}` is not supported in v1".to_string());
            }
            let mut parts = Vec::with_capacity(exprs.len());
            for e in exprs {
                let lowered = lower_constant_expr(e, tree, parameters)?;
                let width = constant_expr_width(&lowered)?;
                parts.push((lowered, width));
            }
            Ok(Expr::Concat(parts))
        }
        other => Err(format!(
            "constant primary form not supported in v1: {other:?}"
        )),
    }
}

/// Attempts to fold an already-lowered `Expr` down to a plain constant --
/// used both by `lower_constant_param_expression` (where the result is
/// guaranteed constant by construction, since the constant-expression
/// grammar has no way to reference a signal at all) and, more
/// significantly, by the handful of places that lower a value through the
/// *general* expression grammar (`lower_expr`) but still require a
/// compile-time constant -- most notably a bit-select target's index
/// (`x[regindex_bits-1] <= v;`, picorv32's own real case: `regindex_bits`
/// resolves to `Expr::Literal` via `lower_primary`'s parameter fallback,
/// so the whole `Expr::Sub(Literal, Literal)` tree this folds is
/// constant, even though it was built by the same general-purpose path
/// that would just as happily build `Expr::Sub(Ref(signal), Literal)` for
/// a genuinely non-constant index). `Expr::Ref` and `Expr::DynamicBitSelect`
/// are the only two forms that return `None` -- a real signal value, or an
/// index that depends on one, simply isn't known until simulation runs.
/// Every other `Expr` variant is handled, deliberately exhaustively (no
/// wildcard arm): this mirrors `ictus_kernel::eval_expr`'s own logic
/// (duplicated rather than shared across the crate boundary -- constant
/// folding is a frontend/elaboration-time concern, evaluation is the
/// kernel's runtime concern; different phases, so some overlap is
/// expected, not a sign the crates should be coupled), so if a new `Expr`
/// variant is ever added, this fails to compile until someone decides
/// whether it can be constant-folded, rather than silently defaulting to
/// "not foldable."
fn try_const_fold(expr: &Expr) -> Option<u64> {
    let mask = |value: u64, width: u32| {
        if width >= 64 {
            value
        } else {
            value & ((1u64 << width) - 1)
        }
    };
    let bool_val = |b: bool| u64::from(b);

    match expr {
        Expr::Literal { value, .. } => Some(*value),
        // An array element's value comes from simulation state, so it's
        // never a compile-time constant -- not even with a constant index.
        Expr::Ref(_) | Expr::DynamicBitSelect { .. } | Expr::ArrayRead { .. } => None,
        Expr::Not(inner) => Some(bool_val(try_const_fold(inner)? == 0)),
        Expr::BitwiseNot(inner, width) => Some(mask(!try_const_fold(inner)?, *width)),
        Expr::ReduceAnd(inner, width) => {
            let value = mask(try_const_fold(inner)?, *width);
            Some(bool_val(value == mask(u64::MAX, *width)))
        }
        Expr::ReduceOr(inner, width) => Some(bool_val(mask(try_const_fold(inner)?, *width) != 0)),
        Expr::ReduceXor(inner, width) => {
            let value = mask(try_const_fold(inner)?, *width);
            Some(bool_val(value.count_ones() % 2 == 1))
        }
        Expr::Add(lhs, rhs) => Some(try_const_fold(lhs)?.wrapping_add(try_const_fold(rhs)?)),
        Expr::Sub(lhs, rhs) => Some(try_const_fold(lhs)?.wrapping_sub(try_const_fold(rhs)?)),
        Expr::Mul(lhs, rhs) => Some(try_const_fold(lhs)?.wrapping_mul(try_const_fold(rhs)?)),
        // Same out-of-range-shift rules as `ictus_kernel::eval_expr`'s own
        // arms for these (64-or-more shifts everything out for a logical
        // shift; an arithmetic one clamps to 63, which already replicates
        // the sign bit everywhere).
        Expr::Shl(lhs, rhs) => {
            let value = try_const_fold(lhs)?;
            let shift = u32::try_from(try_const_fold(rhs)?).ok()?;
            Some(value.checked_shl(shift).unwrap_or(0))
        }
        Expr::Shr(lhs, rhs) => {
            let value = try_const_fold(lhs)?;
            let shift = u32::try_from(try_const_fold(rhs)?).ok()?;
            Some(value.checked_shr(shift).unwrap_or(0))
        }
        Expr::AShr(lhs, rhs) => {
            let value = try_const_fold(lhs)? as i64;
            let shift = u32::try_from(try_const_fold(rhs)?).unwrap_or(u32::MAX).min(63);
            Some((value >> shift) as u64)
        }
        Expr::And(lhs, rhs) => Some(try_const_fold(lhs)? & try_const_fold(rhs)?),
        Expr::Or(lhs, rhs) => Some(try_const_fold(lhs)? | try_const_fold(rhs)?),
        Expr::Xor(lhs, rhs) => Some(try_const_fold(lhs)? ^ try_const_fold(rhs)?),
        Expr::Eq(lhs, rhs) => Some(bool_val(try_const_fold(lhs)? == try_const_fold(rhs)?)),
        Expr::Ne(lhs, rhs) => Some(bool_val(try_const_fold(lhs)? != try_const_fold(rhs)?)),
        Expr::Lt(lhs, rhs) => Some(bool_val(try_const_fold(lhs)? < try_const_fold(rhs)?)),
        Expr::Le(lhs, rhs) => Some(bool_val(try_const_fold(lhs)? <= try_const_fold(rhs)?)),
        Expr::Gt(lhs, rhs) => Some(bool_val(try_const_fold(lhs)? > try_const_fold(rhs)?)),
        Expr::Ge(lhs, rhs) => Some(bool_val(try_const_fold(lhs)? >= try_const_fold(rhs)?)),
        Expr::SignedLt(lhs, rhs) => Some(bool_val(
            (try_const_fold(lhs)? as i64) < (try_const_fold(rhs)? as i64),
        )),
        Expr::LogicalAnd(lhs, rhs) => {
            Some(bool_val(try_const_fold(lhs)? != 0 && try_const_fold(rhs)? != 0))
        }
        Expr::LogicalOr(lhs, rhs) => {
            Some(bool_val(try_const_fold(lhs)? != 0 || try_const_fold(rhs)? != 0))
        }
        Expr::Select { base, msb, lsb } => Some(mask(try_const_fold(base)? >> lsb, msb - lsb + 1)),
        Expr::Concat(parts) => {
            let mut result = 0u64;
            for (part, width) in parts {
                result = (result << width) | mask(try_const_fold(part)?, *width);
            }
            Some(result)
        }
        Expr::Ternary { cond, then_val, else_val } => {
            if try_const_fold(cond)? != 0 {
                try_const_fold(then_val)
            } else {
                try_const_fold(else_val)
            }
        }
        Expr::Signed(inner, width) => {
            let width = *width;
            let value = try_const_fold(inner)?;
            if width == 0 || width >= 64 {
                Some(value)
            } else {
                let value = mask(value, width);
                if (value >> (width - 1)) & 1 == 1 {
                    Some(value | (u64::MAX << width))
                } else {
                    Some(value)
                }
            }
        }
    }
}

/// Like `expr_width`, but for an `Expr` built entirely by
/// `lower_constant_expr`/`lower_constant_primary` -- which never produce
/// `Expr::Ref` (a constant expression can only reference an
/// already-resolved parameter, which always becomes `Expr::Literal`), so
/// no `Module` is ever needed here to look up a signal's width the way
/// `expr_width` needs one for the general case.
fn constant_expr_width(expr: &Expr) -> Result<u32, String> {
    match expr {
        Expr::Literal { width, .. } => Ok(*width),
        Expr::Concat(parts) => Ok(parts.iter().map(|(_, w)| w).sum()),
        Expr::Ternary { then_val, else_val, .. } => {
            Ok(constant_expr_width(then_val)?.max(constant_expr_width(else_val)?))
        }
        other => Err(format!(
            "concatenation operand's width can't be determined in a constant expression in v1: \
             {other:?}"
        )),
    }
}

/// Scans every `task ... endtask` declaration in the module and returns
/// the names of the ones with a *provably empty* body -- every one of its
/// top-level statements is a no-op per `statement_is_noop` (recursing
/// into `begin...end` blocks, since picorv32's own `empty_statement` task
/// is written as `begin end`, not literally zero top-level statements;
/// local variable/port declarations inside the task don't affect this
/// check either way, since declaring a local does nothing observable on
/// its own). This is the only thing v1 ever learns about a task: it
/// doesn't model task execution, ports, or local state at all, so a call
/// is only ever accepted (as a no-op -- see `lower_task_call_statement`)
/// when the task's body is provably empty; a task with any real statement
/// in it, at any nesting depth, is left out of the returned set entirely;
/// downstream, a call to a name that isn't in the set is rejected the
/// same way whether that's because the task has a real body or because
/// no such task exists.
fn lower_task_declarations(
    module_node: &sv_parser::ModuleDeclarationAnsi,
    tree: &SyntaxTree,
) -> Result<HashSet<String>, String> {
    let mut empty_tasks = HashSet::new();

    for node in module_node.into_iter() {
        let RefNode::TaskDeclaration(task) = node else {
            continue;
        };

        let (name_node, is_empty) = match &task.nodes.2 {
            sv_parser::TaskBodyDeclaration::WithoutPort(body) => {
                (&body.nodes.1, body.nodes.4.iter().all(statement_is_noop))
            }
            sv_parser::TaskBodyDeclaration::WithPort(body) => {
                (&body.nodes.1, body.nodes.5.iter().all(statement_is_noop))
            }
        };
        let ident = unwrap_node!(name_node, SimpleIdentifier).ok_or("task identifier unreadable")?;
        let name = ident_str(ident, tree).ok_or("task identifier unreadable")?;

        if is_empty {
            empty_tasks.insert(name.to_string());
        }
    }

    Ok(empty_tasks)
}

/// A statement counts as a no-op for `lower_task_declarations`'s
/// emptiness check if it's a null statement (`;`, with no attributes --
/// attributes can carry tool directives, so one is conservatively treated
/// as *not* provably a no-op) or a `begin...end` block whose own
/// statements are *all*, recursively, no-ops -- picorv32's own
/// `empty_statement` task body is exactly `begin end`, an empty block,
/// not literally zero statements at the task's top level, so checking
/// only `Vec<StatementOrNull>::is_empty()` at that one level would miss
/// it. Any other statement kind (even a single one, nested arbitrarily
/// deep inside otherwise-empty blocks) makes the whole task not provably
/// empty.
fn statement_is_noop(stmt: &StatementOrNull) -> bool {
    match stmt {
        StatementOrNull::Attribute(attr) => attr.nodes.0.is_empty(),
        StatementOrNull::Statement(s) => match &s.nodes.2 {
            StatementItem::SeqBlock(seq) => seq.nodes.3.iter().all(statement_is_noop),
            _ => false,
        },
    }
}

fn ident_str<'a>(node: RefNode<'a>, tree: &'a SyntaxTree) -> Option<&'a str> {
    let locate: &Locate = match node {
        RefNode::SimpleIdentifier(x) => &x.nodes.0,
        RefNode::EscapedIdentifier(x) => &x.nodes.0,
        _ => return None,
    };
    tree.get_str(locate)
}

fn lower_port(
    port: &AnsiPortDeclaration,
    tree: &SyntaxTree,
    last_direction: &mut Option<Direction>,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<Signal, String> {
    let ident_node =
        unwrap_node!(port, PortIdentifier).ok_or("port declaration has no identifier")?;
    let ident_simple =
        unwrap_node!(ident_node, SimpleIdentifier).ok_or("port identifier unreadable")?;
    let name = ident_str(ident_simple, tree)
        .ok_or("could not read port identifier")?
        .to_string();

    let direction = match unwrap_node!(port, PortDirection) {
        Some(RefNode::PortDirection(PortDirection::Input(_))) => Direction::Input,
        Some(RefNode::PortDirection(PortDirection::Output(_))) => Direction::Output,
        Some(_) => {
            return Err(format!(
                "port '{name}' has an unsupported direction (only input/output in v1)"
            ))
        }
        // No direction keyword of its own -- inherit from the previous
        // port in the list (see the comment at this function's call site).
        None => last_direction.ok_or_else(|| {
            format!("port '{name}' has no direction, and there's no previous port to inherit one from")
        })?,
    };
    *last_direction = Some(direction);

    let width = match unwrap_node!(port, PackedDimensionRange) {
        Some(range_node) => lower_packed_range(range_node, tree, parameters)?,
        None => 1,
    };

    // An array *port* would need the array to be addressable from outside
    // the module, which nothing in v1 (no instantiation) can do yet.
    if unwrap_node!(port, UnpackedDimension).is_some() {
        return Err(format!(
            "port '{name}' is an array (an unpacked dimension), which v1 doesn't support -- \
             only an internal `reg`/`wire` can be an array"
        ));
    }

    Ok(Signal {
        name,
        width,
        direction: Some(direction),
        depth: None,
    })
}

/// Lowers a `wire`/`reg` declaration in the module body (as opposed to a
/// port declaration -- see `lower_port`) into one internal `Signal`
/// (`direction: None`) per declared name -- a single declaration can name
/// several (`reg a, b, c;`, real and common: picorv32 itself declares
/// three registers this way in one line). Generic over the declaration's
/// concrete node type (`NetDeclarationNetType` for `wire`,
/// `DataDeclarationVariable` for `reg`) since both are searched the same
/// way: collect every `NetIdentifier`/`VariableIdentifier` node in the
/// declaration's subtree -- these are the grammar's own "this is a
/// declared name" markers, used only in a declaration's own
/// name-and-optional-initializer list, never inside a general expression
/// -- so this can't accidentally pick up an unrelated identifier from an
/// initializer expression the way a blind search for any
/// `SimpleIdentifier` anywhere in the subtree could (e.g. `reg x = Y;`
/// would wrongly also match `Y`). All declared names in one declaration
/// share the same width.
fn lower_internal_signal<'a, T>(
    decl: &'a T,
    tree: &'a SyntaxTree,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<Vec<Signal>, String>
where
    &'a T: IntoIterator<Item = RefNode<'a>>,
{
    let width = match unwrap_node!(decl, PackedDimensionRange) {
        Some(range_node) => lower_packed_range(range_node, tree, parameters)?,
        None => 1,
    };

    // An *unpacked* dimension (`reg [31:0] mem [0:31];`) makes this an
    // array -- a distinct grammar node from the packed one above, so the
    // width search can't confuse the two.
    let depth = match unwrap_node!(decl, UnpackedDimensionRange) {
        Some(RefNode::UnpackedDimensionRange(range)) => {
            let bounds = &range.nodes.0.nodes.1;
            let high = lower_constant_index(&bounds.nodes.0, tree, parameters)?;
            let low = lower_constant_index(&bounds.nodes.2, tree, parameters)?;
            Some(high.abs_diff(low) + 1)
        }
        // `mem [4]` (a size rather than a range) and the other unpacked
        // forms aren't lowered; only `[high:low]` is.
        Some(_) | None => match unwrap_node!(decl, UnpackedDimension) {
            Some(_) => {
                return Err(
                    "only a `[high:low]` unpacked dimension is supported in v1 (not `[size]`, \
                     an associative array, or a queue)"
                        .to_string(),
                )
            }
            None => None,
        },
    };

    let mut names = Vec::new();
    for node in decl {
        let is_declared_name = matches!(
            node,
            RefNode::NetIdentifier(_) | RefNode::VariableIdentifier(_)
        );
        if !is_declared_name {
            continue;
        }
        let simple =
            unwrap_node!(node, SimpleIdentifier).ok_or("declaration identifier unreadable")?;
        let name = ident_str(simple, tree)
            .ok_or("declaration identifier unreadable")?
            .to_string();
        names.push(name);
    }

    if names.is_empty() {
        return Err("declaration has no identifier".to_string());
    }

    // The unpacked dimension is found by searching the whole declaration,
    // which can't tell *which* name it belongs to -- fine for the one-name
    // case (every real array declaration found so far), but
    // `reg [7:0] a, mem [0:3];` would wrongly make `a` an array too.
    // Rejected rather than mis-lowered.
    if depth.is_some() && names.len() > 1 {
        return Err(format!(
            "an array declaration that also declares other names in the same statement \
             (`{}`) is not supported in v1 -- declare the array on its own",
            names.join(", ")
        ));
    }

    Ok(names
        .into_iter()
        .map(|name| Signal {
            name,
            width,
            direction: None,
            depth,
        })
        .collect())
}

/// A packed-dimension bound (`[msb:lsb]`, e.g. a port/`reg`/`wire`
/// declaration's width, or a `parameter`/`localparam`'s own declared
/// width) is Verilog's constant-expression grammar -- both bounds go
/// through the same `lower_constant_expr` walk (and `parameters` table)
/// a `parameter`/`localparam` default value does, so a bound may
/// reference a parameter (picorv32's `reg [regindex_bits-1:0] decoded_rd,
/// decoded_rs1;`, `regindex_bits` itself a `localparam`) and not just a
/// plain literal.
fn lower_packed_range(
    range_node: RefNode,
    tree: &SyntaxTree,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<u32, String> {
    let RefNode::PackedDimensionRange(range) = range_node else {
        return Err(
            "packed dimension form not supported in v1 (only a plain `[msb:lsb]` range)"
                .to_string(),
        );
    };
    let constant_range = &range.nodes.0.nodes.1;
    let msb = lower_constant_expr(&constant_range.nodes.0, tree, parameters)?;
    let lsb = lower_constant_expr(&constant_range.nodes.2, tree, parameters)?;
    let msb = try_const_fold(&msb)
        .ok_or_else(|| "packed range bound does not reduce to a compile-time constant".to_string())?;
    let lsb = try_const_fold(&lsb)
        .ok_or_else(|| "packed range bound does not reduce to a compile-time constant".to_string())?;
    Ok((msb as u32).abs_diff(lsb as u32) + 1)
}

fn lower_always(
    always: &AlwaysConstruct,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Option<ClockedProcess>, String> {
    let edge_node = match unwrap_node!(always, EdgeIdentifier) {
        Some(n) => n,
        // No edge identifier at all (e.g. combinational `always @*`) --
        // not lowered in v1.
        None => return Ok(None),
    };
    let is_posedge = matches!(edge_node, RefNode::EdgeIdentifier(EdgeIdentifier::Posedge(_)));
    if !is_posedge {
        // `negedge`-triggered blocks aren't lowered in v1.
        return Ok(None);
    }

    let event_expr = unwrap_node!(always, EventExpressionExpression)
        .ok_or("posedge with no event expression")?;
    let clock_ident = unwrap_node!(event_expr, HierarchicalIdentifier)
        .ok_or("posedge event has no signal identifier")?;
    let clock_simple =
        unwrap_node!(clock_ident, SimpleIdentifier).ok_or("clock identifier unreadable")?;
    let clock_name = ident_str(clock_simple, tree).ok_or("clock identifier unreadable")?;
    let clock = module
        .signal_id(clock_name)
        .ok_or_else(|| format!("clock signal '{clock_name}' is not a port of this module"))?;

    let stmt_node =
        unwrap_node!(always, StatementOrNull).ok_or("always block has no statement body")?;
    let RefNode::StatementOrNull(stmt_or_null) = stmt_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };
    let body = lower_statement_or_null(stmt_or_null, tree, module)?;

    Ok(Some(ClockedProcess { clock, body }))
}

fn lower_statement_or_null(
    node: &StatementOrNull,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    match node {
        StatementOrNull::Statement(stmt) => lower_statement_item(&stmt.nodes.2, tree, module),
        StatementOrNull::Attribute(_) => {
            Err("a null/attribute-only statement is not supported in v1".to_string())
        }
    }
}

fn lower_statement_item(
    item: &StatementItem,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    match item {
        StatementItem::SeqBlock(seq) => lower_seq_block(seq, tree, module),
        StatementItem::NonblockingAssignment(b) => {
            let (assign, _semicolon) = &**b;
            lower_nonblocking_assign(assign, tree, module)
        }
        StatementItem::ConditionalStatement(cond) => Ok(vec![lower_if(cond, tree, module)?]),
        StatementItem::CaseStatement(case) => Ok(vec![lower_case(case, tree, module)?]),
        StatementItem::BlockingAssignment(b) => {
            let (assign, _semicolon) = &**b;
            lower_blocking_assign(assign, tree, module)
        }
        StatementItem::SubroutineCallStatement(call) => lower_task_call_statement(call, tree, module),
        _ => Err(
            "statement form not supported in v1 (only begin/end blocks, if/else, case, \
             blocking and non-blocking assignment, and a call to a provably-empty \n             task)"
                .to_string(),
        ),
    }
}

/// Lowers a task-call statement (`some_task;`) -- v1 doesn't model task
/// execution at all (no ports, no local state, no statement bodies run),
/// so the only call this can honestly accept is one to a task whose body
/// is *provably empty* (see `lower_task_declarations`), lowered as zero
/// statements (a true no-op) rather than guessed at. picorv32 relies on
/// exactly this: its own `` `assert(...) `` macro expands, when
/// assertions are compiled out, to a call to `empty_statement;`, a
/// deliberately-empty task used as a no-op placeholder. A call to any
/// *other* task -- one with a real body, or one not declared in this
/// module at all (a system task, a task from another scope, `disable`,
/// ...) -- is rejected rather than silently treated as a no-op, since
/// that could just as easily discard real behavior in a different
/// design; so could a call *with* arguments (even to an otherwise-empty
/// task, since v1 has no notion of task ports to bind them to), so that's
/// rejected too.
fn lower_task_call_statement(
    call: &sv_parser::SubroutineCallStatement,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    let sv_parser::SubroutineCallStatement::SubroutineCall(inner) = call else {
        return Err("this task/function call statement form is not supported in v1".to_string());
    };
    let (subroutine_call, _semicolon) = &**inner;
    let sv_parser::SubroutineCall::TfCall(tf_call) = subroutine_call else {
        return Err(
            "only a plain task call is supported in v1 (no system task, method call, or randomize())"
                .to_string(),
        );
    };

    if tf_call.nodes.2.is_some() {
        return Err(
            "a task call with arguments is not supported in v1 (only a call to a \
             provably-empty, zero-argument task)"
                .to_string(),
        );
    }

    let ident = unwrap_node!(&tf_call.nodes.0, SimpleIdentifier)
        .ok_or("task call identifier unreadable")?;
    let name = ident_str(ident, tree).ok_or("task call identifier unreadable")?;

    if module.empty_tasks.contains(name) {
        Ok(Vec::new())
    } else {
        Err(format!(
            "call to task '{name}' is not supported in v1 -- only a call to a task whose body \
             is provably empty (a no-op placeholder, e.g. picorv32's own `empty_statement`) is \
             supported"
        ))
    }
}

fn lower_seq_block(seq: &SeqBlock, tree: &SyntaxTree, module: &Ctx) -> Result<Vec<Stmt>, String> {
    let mut out = Vec::new();
    for stmt in &seq.nodes.3 {
        out.extend(lower_statement_or_null(stmt, tree, module)?);
    }
    Ok(out)
}

/// `else if` chains need no new IR: `if (c1) s1 else if (c2) s2 else s3`
/// lowers to the same nested `Stmt::If` a hand-written
/// `if (c1) s1 else begin if (c2) s2 else s3 end` would -- built by
/// folding `cond_stmt.nodes.4`'s `else if` clauses onto the final `else`
/// (`nodes.5`) from the last clause backward, then wrapping the first
/// `if` around the result.
fn lower_if(cond_stmt: &ConditionalStatement, tree: &SyntaxTree, module: &Ctx) -> Result<Stmt, String> {
    let cond = lower_cond_predicate(&cond_stmt.nodes.2.nodes.1, tree, module)?;
    let then_branch = lower_statement_or_null(&cond_stmt.nodes.3, tree, module)?;

    let mut else_branch = match &cond_stmt.nodes.5 {
        Some((_else_kw, stmt)) => lower_statement_or_null(stmt, tree, module)?,
        None => Vec::new(),
    };
    for (_else_kw, _if_kw, paren, stmt) in cond_stmt.nodes.4.iter().rev() {
        let elseif_cond = lower_cond_predicate(&paren.nodes.1, tree, module)?;
        let elseif_then = lower_statement_or_null(stmt, tree, module)?;
        else_branch = vec![Stmt::If {
            cond: elseif_cond,
            then_branch: elseif_then,
            else_branch,
        }];
    }

    Ok(Stmt::If {
        cond,
        then_branch,
        else_branch,
    })
}

/// Takes a bare `CondPredicate` (not `Paren<CondPredicate>`) since it's
/// shared between `if`'s condition (parenthesized: `if (c) ...`) and the
/// ternary operator's (not: `c ? a : b`) -- callers slice off the `Paren`
/// wrapper themselves when they have one.
fn lower_cond_predicate(
    cond_predicate: &sv_parser::CondPredicate,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    let cond_predicate_node = unwrap_node!(cond_predicate, Expression)
        .ok_or("condition is not a plain expression (cond patterns are not supported in v1)")?;
    let RefNode::Expression(cond_expr) = cond_predicate_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };
    lower_expr(cond_expr, tree, module)
}

/// `casez`/`casex` share `case`'s grammar (`CaseStatementNormal`, just a
/// different `CaseKeyword`). The kernel is 2-state only (decisions.md
/// D6), so the part of real `casez`/`casex` semantics that treats an
/// *unknown* (x/z) selector bit as a wildcard can never actually trigger
/// here -- a 2-state value has no x/z bits to begin with. What's left,
/// and what this lowers, is wildcard bits written directly into a case
/// *item*'s literal (`8'b1010????`): each such item becomes
/// `CaseValue::Wildcard` (see `lower_case_value`); everything else
/// (including every item under plain `case`) is exact-match, unchanged
/// from before. `inside`/pattern-matching case forms
/// (`CaseStatement::Matches`/`Inside`) aren't supported.
fn lower_case(case: &sv_parser::CaseStatement, tree: &SyntaxTree, module: &Ctx) -> Result<Stmt, String> {
    let sv_parser::CaseStatement::Normal(normal) = case else {
        return Err("`inside`/pattern-matching case forms are not supported in v1".to_string());
    };

    let wildcard_mode = !matches!(normal.nodes.1, sv_parser::CaseKeyword::Case(_));

    let selector = lower_expr(&normal.nodes.2.nodes.1.nodes.0, tree, module)?;

    let mut arms = Vec::new();
    let mut default = Vec::new();
    let items = std::iter::once(&normal.nodes.3).chain(normal.nodes.4.iter());
    for item in items {
        match item {
            sv_parser::CaseItem::NonDefault(nd) => {
                let values = nd
                    .nodes
                    .0
                    .contents()
                    .into_iter()
                    .map(|item_expr| lower_case_value(&item_expr.nodes.0, tree, module, wildcard_mode))
                    .collect::<Result<Vec<_>, _>>()?;
                let body = lower_statement_or_null(&nd.nodes.2, tree, module)?;
                arms.push(CaseArm { values, body });
            }
            sv_parser::CaseItem::Default(d) => {
                default = lower_statement_or_null(&d.nodes.2, tree, module)?;
            }
        }
    }

    Ok(Stmt::Case {
        selector,
        arms,
        default,
    })
}

/// Lowers one `casez`/`casex` (or plain `case`) item value. When
/// `wildcard_mode` is set and the item is literally a binary literal
/// (`8'b1010????`), parses it wildcard-aware via `lower_wildcard_binary`;
/// otherwise falls back to plain `lower_expr` (exact match) -- covering
/// both plain `case` entirely, and any `casez`/`casex` item that happens
/// not to be a binary literal (a decimal/hex value with no wildcard bits
/// is still valid there, just always an exact match).
fn lower_case_value(
    expr: &sv_parser::Expression,
    tree: &SyntaxTree,
    module: &Ctx,
    wildcard_mode: bool,
) -> Result<CaseValue, String> {
    if wildcard_mode {
        if let Some(binary) = as_binary_number(expr) {
            let (value, care_mask) = lower_wildcard_binary(binary, tree)?;
            return Ok(CaseValue::Wildcard { value, care_mask });
        }
    }
    Ok(CaseValue::Exact(lower_expr(expr, tree, module)?))
}

fn as_binary_number(expr: &sv_parser::Expression) -> Option<&sv_parser::BinaryNumber> {
    let sv_parser::Expression::Primary(primary) = expr else {
        return None;
    };
    let sv_parser::Primary::PrimaryLiteral(lit) = &**primary else {
        return None;
    };
    let sv_parser::PrimaryLiteral::Number(number) = &**lit else {
        return None;
    };
    let sv_parser::Number::IntegralNumber(integral) = &**number else {
        return None;
    };
    let sv_parser::IntegralNumber::BinaryNumber(binary) = &**integral else {
        return None;
    };
    Some(binary)
}

/// Parses a (possibly wildcard-containing) binary literal digit-by-digit
/// into a `(value, care_mask)` pair -- `u64::from_str_radix`, used for
/// plain binary literals elsewhere in this frontend, rejects `?`/`x`/`z`
/// characters outright, so wildcard literals need their own parser rather
/// than reusing `lower_number`.
fn lower_wildcard_binary(binary: &sv_parser::BinaryNumber, tree: &SyntaxTree) -> Result<(u64, u64), String> {
    let text = locate_text(&binary.nodes.2.nodes.0, tree)?;
    let digits = strip_underscores(text);

    let mut value: u64 = 0;
    let mut care_mask: u64 = 0;
    for ch in digits.chars() {
        value <<= 1;
        care_mask <<= 1;
        match ch {
            '0' => {
                care_mask |= 1;
            }
            '1' => {
                value |= 1;
                care_mask |= 1;
            }
            '?' | 'z' | 'Z' | 'x' | 'X' => {}
            other => {
                return Err(format!(
                    "unexpected character '{other}' in binary literal '{digits}'"
                ))
            }
        }
    }
    Ok((value, care_mask))
}

fn lower_nonblocking_assign(
    assign: &NonblockingAssignment,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    lower_procedural_assign(&assign.nodes.0, &assign.nodes.3, false, tree, module)
}

/// Lowers a *blocking* assignment (`x = value;`). Only the plain `=` form
/// is accepted: sv-parser routes `=` and the compound operators
/// (`+=`, `<<=`, ...) through the same `OperatorAssignment` node, and a
/// compound one is rejected rather than silently treated as a plain
/// assignment, which would discard the read-modify part entirely.
fn lower_blocking_assign(
    assign: &sv_parser::BlockingAssignment,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    let sv_parser::BlockingAssignment::OperatorAssignment(op_assign) = assign else {
        return Err(
            "this blocking assignment form is not supported in v1 (no delay or event control \
             on the assignment itself)"
                .to_string(),
        );
    };
    let operator =
        symbol_text(&op_assign.nodes.1, tree).ok_or("assignment operator unreadable")?;
    if operator != "=" {
        return Err(format!(
            "compound assignment '{operator}' is not supported in v1 -- write it out as \
             `x = x {} ...;`",
            operator.trim_end_matches('=')
        ));
    }
    lower_procedural_assign(&op_assign.nodes.0, &op_assign.nodes.2, true, tree, module)
}

/// Shared body for both procedural assignment kinds. They differ only in
/// *when* the write lands (see `ictus_ir::Stmt::BlockingAssign`), never in
/// what a target may look like, so target resolution -- concatenation,
/// array element, bit range, or plain signal -- is written once here and
/// the `blocking` flag only picks which `Stmt` comes out.
fn lower_procedural_assign(
    lvalue: &sv_parser::VariableLvalue,
    value_expr: &sv_parser::Expression,
    blocking: bool,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    // A concatenation target (`{a, b} <= x;`) is its own `VariableLvalue`
    // variant (`Lvalue`, wrapping a brace-list of lvalues), not just an
    // identifier with an unusual `Select` -- checked separately, and
    // first, because the identifier-based checks below would otherwise
    // deep-search *past* this and silently match `a` alone, discarding
    // `b` and the split-assignment semantics entirely. Handled by
    // `lower_concat_target_assign`, which splits it into one statement
    // per part rather than needing new IR.
    if let sv_parser::VariableLvalue::Lvalue(concat) = lvalue {
        let value = lower_expr(value_expr, tree, module)?;
        return lower_concat_target_assign(concat, value, blocking, tree, module);
    }

    let lhs_ident = unwrap_node!(lvalue, SimpleIdentifier)
        .ok_or("assignment target is not a simple identifier")?;
    let target_name = ident_str(lhs_ident, tree).ok_or("assignment target unreadable")?;
    let target = module
        .signal_id(target_name)
        .ok_or_else(|| format!("assignment target '{target_name}' is not a known signal"))?;

    // Writing an array element (`mem[i] <= v;`) addresses a chosen
    // element rather than a bit range of a fixed signal, so it's its own
    // statement -- see `ictus_ir::Stmt::ArrayAssign`.
    if module.signals[target].depth.is_some() {
        let select = unwrap_node!(lvalue, Select)
            .and_then(|node| match node {
                RefNode::Select(select) => Some(select),
                _ => None,
            })
            .ok_or_else(|| format!("array '{target_name}' is written without an index"))?;
        let Expr::ArrayRead { array, index } = lower_array_index(select, target, tree, module)?
        else {
            unreachable!("lower_array_index always returns an ArrayRead");
        };
        let index = *index;
        let value = lower_expr(value_expr, tree, module)?;
        return Ok(vec![if blocking {
            Stmt::BlockingArrayAssign { array, index, value }
        } else {
            Stmt::ArrayAssign { array, index, value }
        }]);
    }

    let target_range = lower_select_target_range(lvalue, target_name, tree, module)?;
    check_target_range(target_range, target, target_name, module)?;

    let value = lower_expr(value_expr, tree, module)?;

    Ok(vec![if blocking {
        Stmt::BlockingAssign {
            target,
            target_range,
            value,
        }
    } else {
        Stmt::NonBlockingAssign {
            target,
            target_range,
            value,
        }
    }])
}

/// Lowers `{a, b[3:0], ...} <= value;` -- a concatenation used as a
/// non-blocking assignment target (picorv32 does this for its
/// instruction-decode registers, e.g. `{mem_rdata_q[31:25],
/// mem_rdata_q[11:7]} <= {...};`) -- into one plain
/// `Stmt::NonBlockingAssign` per part, each writing the slice of `value`
/// that lines up with that part's position in the concatenation: the
/// *leftmost* part gets the most-significant bits, exactly like a
/// concatenation *expression* packs its parts (see `lower_concatenation`
/// and `ictus_ir::Expr::Concat`'s doc comment) -- just run in reverse,
/// unpacking instead of packing. No new IR needed: each part's slice is
/// expressed by wrapping the (shared, cloned) `value` expression in an
/// `Expr::Select` for that part's bit range -- correct because a
/// non-blocking assignment's right-hand side has no side effects to
/// worry about duplicating, just a value the kernel re-reads once per
/// part at commit time. Each part must itself be a plain identifier, with
/// an optional *constant* bit-select/part-select (reusing
/// `lower_select_target_range`, the same restriction a non-concatenation
/// select target has) -- a nested concatenation, streaming concatenation,
/// or assignment-pattern part is rejected, not guessed at.
/// (target signal, optional constant select range, this part's width).
type ConcatTargetPart = (SignalId, Option<(u32, u32)>, u32);

fn lower_concat_target_assign(
    concat: &sv_parser::VariableLvalueLvalue,
    value: Expr,
    blocking: bool,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Vec<Stmt>, String> {
    let parts = concat.nodes.0.nodes.1.contents();
    if parts.is_empty() {
        return Err(
            "an empty concatenation `{}` is not supported in v1 as an assignment target"
                .to_string(),
        );
    }

    let mut resolved: Vec<ConcatTargetPart> = Vec::with_capacity(parts.len());
    for part in parts {
        if !matches!(part, sv_parser::VariableLvalue::Identifier(_)) {
            return Err(
                "a concatenation assignment target may only contain plain signals or \
                 constant bit-selects/part-selects in v1 -- nested concatenation, streaming \
                 concatenation, and assignment-pattern parts are not supported"
                    .to_string(),
            );
        }
        let ident = unwrap_node!(part, SimpleIdentifier)
            .ok_or("concatenation assignment target part is not a simple identifier")?;
        let name =
            ident_str(ident, tree).ok_or("concatenation assignment target part unreadable")?;
        let target = module
            .signal_id(name)
            .ok_or_else(|| format!("assignment target '{name}' is not a known signal"))?;
        let target_range = lower_select_target_range(part, name, tree, module)?;
        check_target_range(target_range, target, name, module)?;
        let width = target_range
            .map(|(msb, lsb)| msb - lsb + 1)
            .unwrap_or(module.signals[target].width);
        resolved.push((target, target_range, width));
    }

    let mut shift: u32 = resolved.iter().map(|(_, _, width)| width).sum();
    let mut stmts = Vec::with_capacity(resolved.len());
    for (target, target_range, width) in resolved {
        let msb = shift - 1;
        let lsb = shift - width;
        shift -= width;
        let part_value = Expr::Select {
            base: Box::new(value.clone()),
            msb,
            lsb,
        };
        stmts.push(if blocking {
            Stmt::BlockingAssign {
                target,
                target_range,
                value: part_value,
            }
        } else {
            Stmt::NonBlockingAssign {
                target,
                target_range,
                value: part_value,
            }
        });
    }
    Ok(stmts)
}

/// Lowers `assign target = expr;` (the `Net`-targeted form -- `assign`ing
/// to a `reg`/variable via `ContinuousAssignVariable` is legal SV but
/// rare and not lowered in v1). Only handles a single plain-identifier
/// target: `assign a = x, b = y;` (comma-joined multiple assignments)
/// isn't supported -- `NetAssignment` is found via a subtree search
/// rather than hand-decoding `ListOfNetAssignments`' `List<Symbol,
/// NetAssignment>` wrapper, so a comma-joined statement would silently
/// only lower its first assignment; that's an acceptable v1 gap since
/// it's an unusual style, not a silent-wrong-*value* bug like the ones
/// this frontend's tests specifically guard against. Bit-select and
/// concatenation targets (`assign {a,b} = x;`) are explicitly rejected,
/// not just undocumented gaps -- see the `NetLvalue::Lvalue` check below.
fn lower_continuous_assign(
    net: &ContinuousAssignNet,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Assign, String> {
    let assignment_node =
        unwrap_node!(net, NetAssignment).ok_or("assign statement has no assignment")?;
    let RefNode::NetAssignment(assignment) = assignment_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };

    // Same reasoning as lower_nonblocking_assign's identical check: a
    // concatenation target is its own NetLvalue variant, not something
    // the identifier/Select checks below would otherwise catch.
    if let sv_parser::NetLvalue::Lvalue(_) = &assignment.nodes.0 {
        return Err(
            "assign target is a concatenation (`assign {a, b} = ...`), which v1 doesn't support as a write target"
                .to_string(),
        );
    }

    let lhs_ident = unwrap_node!(&assignment.nodes.0, SimpleIdentifier)
        .ok_or("assign target is not a simple identifier")?;
    let target_name = ident_str(lhs_ident, tree).ok_or("assign target unreadable")?;
    // Continuous assignment (`ictus_ir::Assign`) has no partial-write
    // representation -- unlike non-blocking assignment, it's not deferred
    // to a commit phase, so read-modify-write would need to happen inline
    // against `Simulation::settle_combinational`'s single pass, which
    // isn't implemented. A constant bit-select/part-select target is
    // therefore still rejected here, even though it's now accepted for
    // `<=`.
    if lower_select_target_range(&assignment.nodes.0, target_name, tree, module)?.is_some() {
        return Err(format!(
            "assign target '{target_name}' uses a bit-select/part-select, which v1 doesn't support as a continuous-assignment write target (only for non-blocking `<=`)"
        ));
    }
    let target = module
        .signal_id(target_name)
        .ok_or_else(|| format!("assign target '{target_name}' is not a known signal"))?;
    // Same reasoning as the bit-select case just above: `ictus_ir::Assign`
    // names one whole signal, with no element index to carry, and
    // `settle_combinational` has no commit phase to resolve one in.
    if module.signals[target].depth.is_some() {
        return Err(format!(
            "assign target '{target_name}' is an array element, which v1 doesn't support as a \
             continuous-assignment write target (only for non-blocking `<=`)"
        ));
    }

    let value = lower_expr(&assignment.nodes.2, tree, module)?;

    Ok(Assign { target, value })
}

/// Takes the concrete `Expression` type (not a `RefNode`) deliberately:
/// when a `RefNode::Expression` is constructed by hand (as opposed to
/// coming from the tree walker itself), it does *not* auto-flatten into
/// standalone `ExpressionBinary`/`ExpressionUnary` nodes the way
/// walker-produced nodes do -- matching on the concrete enum's own
/// variants directly sidesteps that ambiguity entirely instead of trying
/// to pattern-match an inconsistently-shaped `RefNode`.
fn lower_expr(expr: &sv_parser::Expression, tree: &SyntaxTree, module: &Ctx) -> Result<Expr, String> {
    use sv_parser::Expression as E;
    match expr {
        E::Primary(primary) => lower_primary(primary, tree, module),
        E::Unary(unary) => {
            let op_text = symbol_text(&unary.nodes.0, tree).ok_or("unary operator unreadable")?;
            let operand = lower_primary(&unary.nodes.2, tree, module)?;
            match op_text {
                "!" => Ok(Expr::Not(Box::new(operand))),
                "~" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::BitwiseNot(Box::new(operand), width))
                }
                "&" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::ReduceAnd(Box::new(operand), width))
                }
                "|" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::ReduceOr(Box::new(operand), width))
                }
                "^" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::ReduceXor(Box::new(operand), width))
                }
                // Reduction NAND/NOR/XNOR are just the corresponding
                // reduction op composed with a 1-bit BitwiseNot -- no
                // dedicated Expr variant needed (see their doc comments in
                // ictus_ir).
                "~&" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::BitwiseNot(
                        Box::new(Expr::ReduceAnd(Box::new(operand), width)),
                        1,
                    ))
                }
                "~|" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::BitwiseNot(
                        Box::new(Expr::ReduceOr(Box::new(operand), width)),
                        1,
                    ))
                }
                "~^" | "^~" => {
                    let width = expr_width(&operand, module)?;
                    Ok(Expr::BitwiseNot(
                        Box::new(Expr::ReduceXor(Box::new(operand), width)),
                        1,
                    ))
                }
                other => Err(format!("unsupported unary operator '{other}'")),
            }
        }
        E::Binary(binary) => {
            let op_text = symbol_text(&binary.nodes.1, tree).ok_or("binary operator unreadable")?;

            // sv-parser mis-associates an *unparenthesized* ternary
            // immediately following a binary operator's right operand:
            // `a > c ? a : c` comes back structured as `a > (c ? a : c)`
            // (Binary{op: >, lhs: a, rhs: ConditionalExpression{cond: c,
            // then: a, else: c}}) instead of the only-correct real-Verilog
            // parse `(a > c) ? a : c` -- every operator handled below
            // binds tighter than `?:`, so a bare (no explicit parens)
            // ConditionalExpression can never legitimately be a binary
            // operator's own right operand; a *parenthesized* ternary
            // operand parses as Primary::MintypmaxExpression instead and
            // is unaffected by this check. Confirmed empirically (not
            // assumed) against both the parenthesized and unparenthesized
            // forms before writing this -- see decl_style.rs and
            // ternary_precedence.rs. Only the immediate-RHS case is fixed
            // here; a ternary appearing deeper on the right (e.g.
            // `a > b + (c ? x : y)`, sic -- without parens there this
            // mis-parses too, one level down) is corrected by this same
            // check firing again during the recursive lower_expr call for
            // that inner operator, but a case where the *outer* operator
            // would ALSO need to re-associate past an already-fixed inner
            // ternary is not handled.
            if let sv_parser::Expression::ConditionalExpression(ternary) = &binary.nodes.3 {
                let lhs = lower_expr(&binary.nodes.0, tree, module)?;
                let inner_cond = lower_cond_predicate(&ternary.nodes.0, tree, module)?;
                let cond = apply_binary_op(op_text, lhs, inner_cond)?;
                let then_val = lower_expr(&ternary.nodes.3, tree, module)?;
                let else_val = lower_expr(&ternary.nodes.5, tree, module)?;
                return Ok(Expr::Ternary {
                    cond: Box::new(cond),
                    then_val: Box::new(then_val),
                    else_val: Box::new(else_val),
                });
            }

            let lhs = lower_expr(&binary.nodes.0, tree, module)?;
            let rhs = lower_expr(&binary.nodes.3, tree, module)?;
            apply_binary_op(op_text, lhs, rhs)
        }
        E::ConditionalExpression(ternary) => {
            let cond = lower_cond_predicate(&ternary.nodes.0, tree, module)?;
            let then_val = lower_expr(&ternary.nodes.3, tree, module)?;
            let else_val = lower_expr(&ternary.nodes.5, tree, module)?;
            Ok(Expr::Ternary {
                cond: Box::new(cond),
                then_val: Box::new(then_val),
                else_val: Box::new(else_val),
            })
        }
        other => Err(format!("expression form not supported in v1: {other:?}")),
    }
}

/// Combines two already-lowered operands with a binary operator.
///
/// A `$signed(...)` operand (`Expr::Signed`) is safe to pass straight
/// through to `+ & | ^ == != && ||` unmodified -- two's-complement
/// addition, bitwise operators, and equality are bit-identical whether
/// the operands are "meant" as signed or unsigned, as long as they're
/// already extended to a common width, which `Expr::Signed`'s own
/// evaluation (see `ictus_kernel::eval_expr`) guarantees. `-` and `*`
/// (added alongside this guard) are safe the same way: two's-complement
/// subtraction and multiplication are *also* bit-identical regardless of
/// declared signedness, as long as only the low bits of a wrapping result
/// are kept -- which is exactly what `wrapping_sub`/`wrapping_mul` on a
/// full `u64` representation, truncated later by whatever narrower
/// context actually needs it, already does.
///
/// Ordering comparisons (`< <= > >=`) are different, and split three ways
/// by how many of their operands are `Signed`. With **both** signed, this
/// builds a real signed comparison out of the single `Expr::SignedLt`
/// variant -- `a > b` is `b < a`, `a <= b` is `!(b < a)`, `a >= b` is
/// `!(a < b)` -- correct as an `i64` comparison downstream precisely
/// because both operands arrive sign-extended across all 64 bits. With
/// **neither** signed, the ordinary unsigned comparison. With exactly
/// **one**, Verilog's own rule is to compare unsigned, but that needs the
/// signed operand truncated back to its own width first (an 8-bit -1 has
/// to read as 255, not as the 64-bit sign-extended pattern `Expr::Signed`
/// evaluates to) -- not implemented, and not needed by any real design
/// yet, so that combination is rejected rather than silently comparing
/// the sign-extended pattern as an enormous positive number.
///
/// The shift operators split three ways along the same axis. `<<`/`<<<`
/// are sign-agnostic (shifting left has no sign behavior to differ
/// about), so a `Signed` operand passes through like `+`/`-`/`*` do.
/// `>>` is a *logical* shift in Verilog even for a signed operand, which
/// a sign-extended `Expr::Signed` value can't represent (its extension
/// bits would shift down in place of the zeros Verilog wants), so that
/// combination is rejected like an ordering comparison. `>>>` is the one
/// that genuinely needs the distinction, and it's the one place a
/// `Signed` operand is actively *required* rather than tolerated: with
/// one, it lowers to `Expr::AShr` (correct precisely because the operand
/// arrives already sign-extended); without one, Verilog itself defines
/// `>>>` as an ordinary logical shift, so it lowers to `Expr::Shr`.
/// Division would still have the ordering-comparison problem if it were
/// ever added -- it isn't implemented at all, so it stays safely (if
/// incidentally) rejected by the `other` arm below.
fn apply_binary_op(op_text: &str, lhs: Expr, rhs: Expr) -> Result<Expr, String> {
    let is_ordering_comparison = matches!(op_text, "<" | "<=" | ">" | ">=");
    if is_ordering_comparison {
        let lhs_signed = matches!(lhs, Expr::Signed(..));
        let rhs_signed = matches!(rhs, Expr::Signed(..));
        match (lhs_signed, rhs_signed) {
            // Both signed: a real signed comparison, built from the one
            // `SignedLt` variant plus operand-swapping and negation.
            (true, true) => {
                return Ok(match op_text {
                    "<" => Expr::SignedLt(Box::new(lhs), Box::new(rhs)),
                    ">" => Expr::SignedLt(Box::new(rhs), Box::new(lhs)),
                    "<=" => Expr::Not(Box::new(Expr::SignedLt(Box::new(rhs), Box::new(lhs)))),
                    ">=" => Expr::Not(Box::new(Expr::SignedLt(Box::new(lhs), Box::new(rhs)))),
                    _ => unreachable!("is_ordering_comparison covers exactly these four"),
                });
            }
            // Exactly one signed: Verilog says compare *unsigned* here --
            // but doing that correctly needs the signed operand
            // re-truncated to its own width first (an 8-bit -1 has to read
            // as 255, not as the 64-bit sign-extended pattern
            // `Expr::Signed` evaluates to). Not implemented, and no real
            // design has needed it, so it's rejected rather than silently
            // comparing that huge pattern.
            (true, false) | (false, true) => {
                return Err(
                    "a mixed signed/unsigned comparison (only one operand is $signed(...)) is \
                     not supported in v1 -- Verilog compares these as *unsigned*, which needs \
                     the signed operand truncated back to its own width first"
                        .to_string(),
                );
            }
            // Neither signed: the ordinary unsigned comparison below.
            (false, false) => {}
        }
    }

    // Verilog's `>>` zero-fills for a signed operand just as it does for
    // an unsigned one (that difference is exactly what `>>>` exists for),
    // but `Expr::Signed` evaluates to a value already sign-extended across
    // all 64 bits -- so a logical shift of it would pull those extension
    // bits down into the result instead of zeros. Rejected rather than
    // silently producing that, the same way an ordering comparison of a
    // `Signed` operand is above; no real design has needed it yet.
    if op_text == ">>" && matches!(lhs, Expr::Signed(..)) {
        return Err(
            "a logical right shift of a $signed(...) value (`$signed(x) >> n`) is not \
             supported in v1 -- use `>>>` for an arithmetic (sign-replicating) shift, which \
             is what a signed operand almost always means"
                .to_string(),
        );
    }

    match op_text {
        "+" => Ok(Expr::Add(Box::new(lhs), Box::new(rhs))),
        "-" => Ok(Expr::Sub(Box::new(lhs), Box::new(rhs))),
        "*" => Ok(Expr::Mul(Box::new(lhs), Box::new(rhs))),
        // `<<<` is bit-for-bit identical to `<<` in Verilog -- shifting
        // left has no sign behavior to differ about -- so both lower to
        // the same node rather than getting a distinct one.
        "<<" | "<<<" => Ok(Expr::Shl(Box::new(lhs), Box::new(rhs))),
        ">>" => Ok(Expr::Shr(Box::new(lhs), Box::new(rhs))),
        // `>>>` differs from `>>` *only* for a signed left operand; on an
        // unsigned one Verilog defines it as an ordinary logical shift, so
        // that's what it lowers to -- not an approximation, the LRM's own
        // rule. The `Signed` check is deliberately shallow (the operand
        // itself, not a search through it), matching the ordering-comparison
        // guard above: picorv32 always writes the direct
        // `$signed(...) >>> n` form, so a `Signed` value buried deeper
        // (`($signed(a) + 1) >>> n`) isn't detected -- and would lower to a
        // logical shift. Worth revisiting if a real design ever writes that.
        ">>>" => {
            if matches!(lhs, Expr::Signed(..)) {
                Ok(Expr::AShr(Box::new(lhs), Box::new(rhs)))
            } else {
                Ok(Expr::Shr(Box::new(lhs), Box::new(rhs)))
            }
        }
        "&" => Ok(Expr::And(Box::new(lhs), Box::new(rhs))),
        "|" => Ok(Expr::Or(Box::new(lhs), Box::new(rhs))),
        "^" => Ok(Expr::Xor(Box::new(lhs), Box::new(rhs))),
        "==" => Ok(Expr::Eq(Box::new(lhs), Box::new(rhs))),
        "!=" => Ok(Expr::Ne(Box::new(lhs), Box::new(rhs))),
        "<" => Ok(Expr::Lt(Box::new(lhs), Box::new(rhs))),
        "<=" => Ok(Expr::Le(Box::new(lhs), Box::new(rhs))),
        ">" => Ok(Expr::Gt(Box::new(lhs), Box::new(rhs))),
        ">=" => Ok(Expr::Ge(Box::new(lhs), Box::new(rhs))),
        "&&" => Ok(Expr::LogicalAnd(Box::new(lhs), Box::new(rhs))),
        "||" => Ok(Expr::LogicalOr(Box::new(lhs), Box::new(rhs))),
        other => Err(format!("unsupported binary operator '{other}'")),
    }
}

/// Matches `Primary`'s own variants directly rather than deep-searching
/// its subtree for `Number`/`HierarchicalIdentifier` -- a deep search is
/// too permissive here: for a parenthesized sub-expression like
/// `(a == b)`, searching the whole subtree for the first
/// `HierarchicalIdentifier` finds `a` (nested inside the parenthesized
/// expression) and would wrongly lower the *entire* primary as just a
/// reference to `a`, silently discarding the `== b` part. Matching the
/// immediate variant avoids reaching past the primary's own top level.
fn lower_primary(primary: &sv_parser::Primary, tree: &SyntaxTree, module: &Ctx) -> Result<Expr, String> {
    use sv_parser::Primary as P;
    match primary {
        P::PrimaryLiteral(lit) => match &**lit {
            sv_parser::PrimaryLiteral::Number(number) => lower_number(number, tree),
            other => Err(format!("literal form not supported in v1: {other:?}")),
        },
        P::Hierarchical(h) => {
            let simple = unwrap_node!(&h.nodes.1, SimpleIdentifier)
                .ok_or("identifier reference unreadable")?;
            let name = ident_str(simple, tree).ok_or("identifier reference unreadable")?;
            // A parameter isn't a signal -- it's a compile-time constant,
            // resolved to a plain Literal right here rather than an
            // Expr::Ref, since it has no SignalId and the kernel never
            // needs to know it existed (see lower_parameters).
            let base = if let Some(id) = module.signal_id(name) {
                Expr::Ref(id)
            } else if let Some(&(value, width)) = module.parameters.get(name) {
                Expr::Literal { value, width }
            } else {
                return Err(format!("reference to unknown signal or parameter '{name}'"));
            };
            lower_select(&h.nodes.2, base, tree, module)
        }
        P::MintypmaxExpression(paren) => match &paren.nodes.0.nodes.1 {
            sv_parser::MintypmaxExpression::Expression(inner) => lower_expr(inner, tree, module),
            sv_parser::MintypmaxExpression::Ternary(_) => {
                Err("min:typ:max expressions are not supported in v1".to_string())
            }
        },
        P::Concatenation(concat) => {
            if concat.nodes.1.is_some() {
                return Err(
                    "indexing into a concatenation (`{a,b}[3:0]`) is not supported in v1".to_string(),
                );
            }
            lower_concatenation(&concat.nodes.0, tree, module)
        }
        P::MultipleConcatenation(mc) => {
            if mc.nodes.1.is_some() {
                return Err(
                    "indexing into a replication concatenation (`{4{a}}[3:0]`) is not supported in v1"
                        .to_string(),
                );
            }
            lower_multiple_concatenation(&mc.nodes.0, tree, module)
        }
        P::FunctionSubroutineCall(call) => lower_system_function_call(call, tree, module),
        other => Err(format!("primary expression form not supported in v1: {other:?}")),
    }
}

/// Lowers a system function/task call -- v1 only supports `$signed(expr)`
/// (see `ictus_ir::Expr::Signed`'s doc comment for its semantics and why
/// v1 needs it: picorv32 uses it throughout for RISC-V immediate
/// sign-extension, e.g. `decoded_imm <= $signed(mem_rdata_q[31:20]);`).
/// Every other system function (`$unsigned`, `$display`, ...) and every
/// other `SubroutineCall` form (a plain task/function call, a method
/// call, `randomize()`) is rejected, not guessed at.
fn lower_system_function_call(
    call: &sv_parser::FunctionSubroutineCall,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    let sv_parser::SubroutineCall::SystemTfCall(sys) = &call.nodes.0 else {
        return Err(
            "only the $signed system function is supported in v1 (no plain task/function \
             calls, method calls, or randomize())"
                .to_string(),
        );
    };
    let sv_parser::SystemTfCall::ArgExpression(args) = &**sys else {
        return Err(
            "this system function form is not supported in v1 (only $signed(expr), a single \
             plain-expression argument)"
                .to_string(),
        );
    };

    let name = tree
        .get_str(&args.nodes.0.nodes.0)
        .ok_or("system function name unreadable")?;
    if name != "$signed" {
        return Err(format!(
            "system function '{name}' is not supported in v1 (only $signed is)"
        ));
    }

    let arguments = args.nodes.1.nodes.1.0.contents();
    if arguments.len() != 1 {
        return Err("$signed(...) must take exactly one argument in v1".to_string());
    }
    let argument = arguments[0]
        .as_ref()
        .ok_or("$signed(...) argument is missing")?;

    let inner = lower_expr(argument, tree, module)?;
    let width = expr_width(&inner, module)?;
    Ok(Expr::Signed(Box::new(inner), width))
}

/// Applies a `Select` (`x[3]`, `x[i]`, `x[7:0]`, or neither for a plain
/// reference) to an already-lowered `base` expression. Part-select bounds
/// (`x[7:0]`) must still be constants known at lowering time --
/// `PartSelectRange::ConstantRange` only, not `IndexedRange`
/// (`x[base +: width]`, a variable base with fixed width), which isn't
/// supported yet. A single bit-select's index (`x[3]` or `x[i]`) can be
/// anything: an expression that folds to a constant at lowering time
/// (via `try_const_fold` -- not just a bare literal like `3`, but
/// anything built from literals and already-resolved parameter
/// references, e.g. `regindex_bits-1`) lowers to `Expr::Select`, same as
/// before; anything that doesn't fold (a genuine signal reference
/// somewhere in the index) lowers to `Expr::DynamicBitSelect` and is
/// evaluated fresh at simulation time instead.
fn lower_select(
    select: &sv_parser::Select,
    base: Expr,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    // An index on an *array* signal selects an element, not a bit -- and
    // that's true whether or not the index is constant, unlike a
    // bit-select. Checked before the bit-select handling below, which
    // would otherwise read `mem[3]` as bit 3 of `mem`.
    if let Expr::Ref(id) = base {
        if module.signals[id].depth.is_some() {
            return lower_array_index(select, id, tree, module);
        }
    }

    // Part-select: `x[msb:lsb]`.
    if let Some(bracket) = &select.nodes.2 {
        return match &bracket.nodes.1 {
            sv_parser::PartSelectRange::ConstantRange(range) => {
                let msb = lower_constant_index(&range.nodes.0, tree, module.parameters)?;
                let lsb = lower_constant_index(&range.nodes.2, tree, module.parameters)?;
                if lsb > msb {
                    return Err(format!("part-select `[{msb}:{lsb}]` has lsb greater than msb"));
                }
                Ok(Expr::Select {
                    base: Box::new(base),
                    msb,
                    lsb,
                })
            }
            sv_parser::PartSelectRange::IndexedRange(_) => Err(
                "indexed part-select (`x[base +: width]`/`x[base -: width]`) is not supported in v1"
                    .to_string(),
            ),
        };
    }

    // Bit-select: `x[3]` (or no select at all, if the bracket list is empty).
    match select.nodes.1.nodes.0.as_slice() {
        [] => Ok(base),
        [only] => {
            let index = lower_expr(&only.nodes.1, tree, module)?;
            match try_const_fold(&index) {
                Some(value) => {
                    let bit = value as u32;
                    Ok(Expr::Select {
                        base: Box::new(base),
                        msb: bit,
                        lsb: bit,
                    })
                }
                // Not foldable at lowering time (a genuine signal
                // reference somewhere in the index expression) -- evaluated
                // fresh each simulation cycle instead.
                None => Ok(Expr::DynamicBitSelect {
                    base: Box::new(base),
                    index: Box::new(index),
                }),
            }
        }
        _ => Err("multi-dimensional array indexing is not supported in v1".to_string()),
    }
}

/// Lowers an index applied to an *array* signal (`mem[i]`) into an
/// element read. Shared by the read side (`lower_select`) and the write
/// side (`lower_array_target`), which need the same index expression and
/// the same restrictions -- exactly one index (no multi-dimensional
/// array), and no bit range layered on top of the element, since
/// `Stmt::ArrayAssign` can't represent a partial element write and
/// letting it lower on the read side alone would be a confusing
/// asymmetry.
fn lower_array_index(
    select: &sv_parser::Select,
    array: SignalId,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    let name = &module.signals[array].name;
    if select.nodes.2.is_some() {
        return Err(format!(
            "a bit-select/part-select of an array element (`{name}[i][n:m]`) is not supported \
             in v1 -- read or write the whole element"
        ));
    }
    match select.nodes.1.nodes.0.as_slice() {
        [] => Err(format!(
            "array '{name}' is used without an index -- v1 has no whole-array read or write, \
             only `{name}[i]`"
        )),
        [only] => Ok(Expr::ArrayRead {
            array,
            index: Box::new(lower_expr(&only.nodes.1, tree, module)?),
        }),
        // A second index on a one-dimensional array is a bit-select of the
        // chosen element (`mem[i][3]`) -- the parser can't tell that apart
        // from a genuinely multi-dimensional array, and neither is
        // supported, so the message covers both readings.
        _ => Err(format!(
            "indexing array element '{name}[i]' further (a bit-select of the element, or a \
             multi-dimensional array) is not supported in v1"
        )),
    }
}

/// A bit-select/part-select bound (`x[7:0]`) is the same constant-
/// expression grammar a `parameter`/`localparam` default or a
/// packed-range bound uses -- see `lower_constant_expr`, which this
/// reuses rather than keeping a separate, less capable evaluator.
fn lower_constant_index(
    expr: &sv_parser::ConstantExpression,
    tree: &SyntaxTree,
    parameters: &HashMap<String, (u64, u32)>,
) -> Result<u32, String> {
    let folded = lower_constant_expr(expr, tree, parameters)?;
    try_const_fold(&folded)
        .map(|value| value as u32)
        .ok_or_else(|| {
            "a bit-select/part-select bound or array dimension does not reduce to a \
             compile-time constant"
                .to_string()
        })
}

fn lower_concatenation(
    concat: &sv_parser::Concatenation,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    let exprs = concat.nodes.0.nodes.1.contents();
    if exprs.is_empty() {
        return Err("an empty concatenation `{}` is not supported in v1".to_string());
    }

    let mut parts = Vec::with_capacity(exprs.len());
    for e in exprs {
        let lowered = lower_expr(e, tree, module)?;
        let width = expr_width(&lowered, module)?;
        parts.push((lowered, width));
    }
    Ok(Expr::Concat(parts))
}

/// Lowers a replication/multiple concatenation (`{4{a}}`, `{2{a,b}}` --
/// Verilog's `{N{...}}` syntax, distinct from the plain `{a,b}`
/// concatenation `lower_concatenation` handles): the inner `{...}` is
/// lowered exactly like any other concatenation, and the count `N` --
/// which must reduce to a compile-time constant, same restriction as a
/// bit-select/part-select bound (see `lower_constant_index`) -- says how
/// many times to repeat its parts. No new IR is needed: since the count
/// is known at lowering time, the result is just a flat `Expr::Concat`
/// whose part list is the inner concatenation's own parts, physically
/// repeated (each repetition an independent clone -- there's nothing
/// shared to alias) `N` times in a row, exactly as if the source had
/// written that many literal copies of `{...}` back to back. A count of
/// `0` is legal Verilog (a deliberate zero-width contribution, typically
/// used inside a *larger* concatenation) but not supported in v1 --
/// `Expr::Concat` can't represent an empty part list any more than a
/// plain `{}` can (see `lower_concatenation`'s identical restriction).
fn lower_multiple_concatenation(
    mc: &sv_parser::MultipleConcatenation,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    let (count_expr, concat) = &mc.nodes.0.nodes.1;

    let count = match lower_expr(count_expr, tree, module)? {
        Expr::Literal { value, .. } => value,
        _ => {
            return Err(
                "a replication count (`{N{...}}`) must be a compile-time constant in v1"
                    .to_string(),
            )
        }
    };
    if count == 0 {
        return Err(
            "a replication count of 0 (`{0{...}}`) is not supported in v1 (Expr::Concat can't \
             represent an empty part list)"
                .to_string(),
        );
    }

    let Expr::Concat(parts) = lower_concatenation(concat, tree, module)? else {
        unreachable!("lower_concatenation always returns Expr::Concat");
    };

    let mut replicated = Vec::with_capacity(parts.len() * count as usize);
    for _ in 0..count {
        replicated.extend(parts.iter().cloned());
    }
    Ok(Expr::Concat(replicated))
}

/// Computes a statically-known bit width for an already-lowered
/// expression -- needed to pack concatenation operands into their correct
/// bit positions (see `Expr::Concat`'s doc comment in `ictus_ir`). Only
/// expression forms with an exactly-known width are accepted:
/// arithmetic/comparison results don't have a width this frontend can
/// determine without real type inference (Verilog's own width-inference
/// rules for something like `a + b` are more involved than this frontend
/// implements), so those are rejected here rather than guessed at.
pub fn expr_width(expr: &Expr, module: &Module) -> Result<u32, String> {
    match expr {
        Expr::Literal { width, .. } => Ok(*width),
        Expr::Ref(id) => Ok(module.signals[*id].width),
        Expr::Select { msb, lsb, .. } => Ok(msb - lsb + 1),
        Expr::DynamicBitSelect { .. } => Ok(1),
        // One element of an array is as wide as the array's element width.
        Expr::ArrayRead { array, .. } => Ok(module.signals[*array].width),
        Expr::Concat(parts) => Ok(parts.iter().map(|(_, w)| w).sum()),
        Expr::Ternary { then_val, else_val, .. } => {
            Ok(expr_width(then_val, module)?.max(expr_width(else_val, module)?))
        }
        // `$signed(...)`'s own natural width, for concatenation-packing
        // purposes, is exactly the width already recorded on it --
        // concatenation uses each operand's *self-determined* width
        // regardless of signedness; only an assignment-like context
        // (outside this function's concern) triggers sign extension.
        Expr::Signed(_, width) => Ok(*width),
        // `BitwiseNot`'s stored width *is* its own result's width (`~x`
        // preserves `x`'s width). A reduction operator's stored width is
        // its *operand's* width (needed for evaluation, see
        // `ictus_kernel::eval_expr`) -- the reduction's own result is
        // always exactly 1 bit, regardless of what it reduced.
        Expr::BitwiseNot(_, width) => Ok(*width),
        // Every comparison, logical, and reduction operator is *defined*
        // by Verilog to produce exactly 1 bit (0 or 1) -- not a guess or
        // an approximation the way a general arithmetic result's width
        // would be, so these are safe to allow as concatenation operands
        // unlike `Add`/`Sub`/`Mul`/etc. below.
        Expr::Not(_)
        | Expr::Eq(..)
        | Expr::Ne(..)
        | Expr::Lt(..)
        | Expr::Le(..)
        | Expr::Gt(..)
        | Expr::Ge(..)
        | Expr::SignedLt(..)
        | Expr::LogicalAnd(..)
        | Expr::LogicalOr(..)
        | Expr::ReduceAnd(..)
        | Expr::ReduceOr(..)
        | Expr::ReduceXor(..) => Ok(1),
        other => Err(format!(
            "concatenation operand's width can't be determined in v1 (only literals, signal \
             references, bit-select/part-select, nested concatenation, ternary, and \
             comparison/logical/reduction operators -- always exactly 1 bit -- are supported \
             as operands): {other:?}"
        )),
    }
}

/// Extracts a `(msb, lsb)` write range from an assignment target that uses
/// a *constant* bit-select/part-select (`x[3:0] <= v;`, `x[5] <= v;`, or
/// `assign x[3:0] = v;`) -- `None` means the target is a plain reference
/// with no select at all, the overwhelmingly common case. `target` is
/// searched for a `Select` node the same way its identifier already is.
///
/// Only constant bounds are supported: a *variable* bit-select target
/// (`x[i] <= v;`, `i` a genuine signal -- checked via `try_const_fold`,
/// the same helper `lower_select`'s read-side bit-select uses, so
/// `x[regindex_bits-1] <= v;` is accepted the same way a read would be)
/// or an indexed part-select (`x[base +: width] <= v;`) would need the
/// kernel to compute the write range at simulation time rather than
/// lowering time, which the kernel doesn't implement -- both are
/// rejected here with a specific error rather than silently doing the
/// wrong thing. Multi-dimensional indexing (array signals) is likewise
/// rejected, matching `lower_select`'s read-side restriction.
fn lower_select_target_range<'a, T>(
    target: &'a T,
    target_name: &str,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Option<(u32, u32)>, String>
where
    &'a T: IntoIterator<Item = RefNode<'a>>,
{
    let Some(RefNode::Select(select)) = unwrap_node!(target, Select) else {
        return Ok(None);
    };

    // Part-select: `x[msb:lsb]`.
    if let Some(bracket) = &select.nodes.2 {
        return match &bracket.nodes.1 {
            sv_parser::PartSelectRange::ConstantRange(range) => {
                let msb = lower_constant_index(&range.nodes.0, tree, module.parameters)?;
                let lsb = lower_constant_index(&range.nodes.2, tree, module.parameters)?;
                if lsb > msb {
                    return Err(format!(
                        "assignment target '{target_name}' has a part-select `[{msb}:{lsb}]` with lsb greater than msb"
                    ));
                }
                Ok(Some((msb, lsb)))
            }
            sv_parser::PartSelectRange::IndexedRange(_) => Err(format!(
                "assignment target '{target_name}' uses an indexed part-select (`x[base +: width]`), which v1 doesn't support as a write target"
            )),
        };
    }

    // Bit-select: `x[3]` (or no select at all, if the bracket list is empty).
    match select.nodes.1.nodes.0.as_slice() {
        [] => Ok(None),
        [only] => {
            let index = lower_expr(&only.nodes.1, tree, module)?;
            match try_const_fold(&index) {
                Some(value) => {
                    let bit = value as u32;
                    Ok(Some((bit, bit)))
                }
                None => Err(format!(
                    "assignment target '{target_name}' uses a variable-indexed bit-select (`x[i] <= v;`), which v1 doesn't support as a write target"
                )),
            }
        }
        _ => Err(format!(
            "assignment target '{target_name}' uses multi-dimensional indexing, which v1 doesn't support"
        )),
    }
}

/// Confirms a target write range's `msb` actually fits inside the target
/// signal's declared width -- `lower_select_target_range` only knows the
/// literal bounds written in the source, not the signal's width, so
/// `x[40:38] <= v;` on an 8-bit `x` needs to be caught here rather than
/// silently accepted and producing an out-of-range write at simulation
/// time.
fn check_target_range(
    range: Option<(u32, u32)>,
    target: SignalId,
    target_name: &str,
    module: &Module,
) -> Result<(), String> {
    let Some((msb, _lsb)) = range else {
        return Ok(());
    };
    let width = module.signals[target].width;
    if msb >= width {
        return Err(format!(
            "assignment target '{target_name}' selects bit {msb}, which is out of range for its {width}-bit width"
        ));
    }
    Ok(())
}

fn symbol_text<'a, T>(op: &'a T, tree: &'a SyntaxTree) -> Option<&'a str>
where
    &'a T: IntoIterator<Item = RefNode<'a>>,
{
    op.into_iter().find_map(|n| match n {
        RefNode::Locate(l) => tree.get_str(l),
        _ => None,
    })
}

/// Takes the concrete `Number` type for the same reason `lower_expr` takes
/// `&Expression` rather than `RefNode`: matching its variants directly is
/// precise, instead of pattern-matching a `RefNode` shape that's only
/// reliable when it comes straight from the tree walker.
fn lower_number(number: &Number, tree: &SyntaxTree) -> Result<Expr, String> {
    let integral = match number {
        Number::IntegralNumber(i) => i,
        Number::RealNumber(_) => return Err("real number literals are not supported".to_string()),
    };
    match &**integral {
        IntegralNumber::DecimalNumber(d) => lower_decimal_number(d, tree),
        IntegralNumber::BinaryNumber(b) => {
            let width = lower_size(&b.nodes.0, tree)?;
            let text = locate_text(&b.nodes.2.nodes.0, tree)?;
            let value = parse_binary_literal_value(&strip_underscores(text))?;
            Ok(Expr::Literal { value, width })
        }
        IntegralNumber::HexNumber(h) => {
            let width = lower_size(&h.nodes.0, tree)?;
            let text = locate_text(&h.nodes.2.nodes.0, tree)?;
            let value = parse_hex_literal_value(&strip_underscores(text))?;
            Ok(Expr::Literal { value, width })
        }
        IntegralNumber::OctalNumber(_) => Err("octal literals are not supported in v1".to_string()),
    }
}

/// A 4-state `x`/`z` digit *outside* a `case`/`casez`/`casex` item's own
/// wildcard matching (see `lower_wildcard_binary`, which this deliberately
/// doesn't share code with -- that one tracks a `care_mask` for pattern
/// matching, this one just needs a plain value) has no real meaning in
/// this 2-state kernel (decisions.md D6) -- resolved to the bit `0`,
/// matching Verilator's own default X-handling policy (a real precedent
/// for exactly this choice, not a guess). See decisions.md D19 for the
/// full reasoning and why this is a deliberate, narrow literal-parsing
/// policy, not the (much larger, still-future) real 4-state signal
/// tracking D6 describes.
fn parse_binary_literal_value(digits: &str) -> Result<u64, String> {
    let mut value: u64 = 0;
    for ch in digits.chars() {
        value <<= 1;
        match ch {
            '0' => {}
            '1' => value |= 1,
            'x' | 'X' | 'z' | 'Z' => {}
            other => {
                return Err(format!(
                    "unexpected character '{other}' in binary literal '{digits}'"
                ))
            }
        }
    }
    Ok(value)
}

/// Same policy as `parse_binary_literal_value` (see its doc comment), one
/// hex digit at a time -- an `x`/`z` hex digit stands for four unknown
/// bits at once, all resolved to `0`.
fn parse_hex_literal_value(digits: &str) -> Result<u64, String> {
    let mut value: u64 = 0;
    for ch in digits.chars() {
        value <<= 4;
        match ch {
            'x' | 'X' | 'z' | 'Z' => {}
            _ => {
                let digit = ch.to_digit(16).ok_or_else(|| {
                    format!("unexpected character '{ch}' in hex literal '{digits}'")
                })?;
                value |= u64::from(digit);
            }
        }
    }
    Ok(value)
}

fn lower_decimal_number(decimal: &DecimalNumber, tree: &SyntaxTree) -> Result<Expr, String> {
    match decimal {
        DecimalNumber::UnsignedNumber(u) => {
            let text = locate_text(&u.nodes.0, tree)?;
            let value = strip_underscores(text)
                .parse::<u64>()
                .map_err(|_| format!("could not parse decimal literal '{text}'"))?;
            Ok(Expr::Literal { value, width: 32 })
        }
        DecimalNumber::BaseUnsigned(b) => {
            let width = lower_size(&b.nodes.0, tree)?;
            let text = locate_text(&b.nodes.2.nodes.0, tree)?;
            let value = strip_underscores(text)
                .parse::<u64>()
                .map_err(|_| format!("could not parse decimal literal '{text}'"))?;
            Ok(Expr::Literal { value, width })
        }
        // A whole-value `x`/`z` decimal literal (`8'dx`/`8'dz` -- decimal
        // 4-state values are all-or-nothing, unlike binary/hex which can
        // mix `x`/`z` with real digits per-bit) -- same "resolves to 0"
        // policy as parse_binary_literal_value/parse_hex_literal_value,
        // just with no per-digit parsing needed since the whole value is
        // already known to be all-`x`/all-`z`.
        DecimalNumber::BaseXNumber(b) => {
            let width = lower_size(&b.nodes.0, tree)?;
            Ok(Expr::Literal { value: 0, width })
        }
        DecimalNumber::BaseZNumber(b) => {
            let width = lower_size(&b.nodes.0, tree)?;
            Ok(Expr::Literal { value: 0, width })
        }
    }
}

fn lower_size(size: &Option<sv_parser::Size>, tree: &SyntaxTree) -> Result<u32, String> {
    match size {
        Some(s) => {
            let text = locate_text(&s.nodes.0.nodes.0, tree)?;
            strip_underscores(text)
                .parse::<u32>()
                .map_err(|_| format!("could not parse literal size '{text}'"))
        }
        None => Ok(32),
    }
}

fn locate_text<'a>(locate: &'a Locate, tree: &'a SyntaxTree) -> Result<&'a str, String> {
    tree.get_str(locate).ok_or_else(|| "could not read literal text".to_string())
}

fn strip_underscores(s: &str) -> String {
    s.chars().filter(|c| *c != '_').collect()
}
