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
//! own literal -- see `lower_case`/`lower_case_value`), non-blocking
//! assignment, internal `wire`/`reg` declarations naming one or more
//! signals per declaration (`reg a, b, c;` -- see `lower_internal_signal`)
//! in addition to ports, module parameters (`#(parameter [7:0] X = 1)`,
//! resolved to plain `Expr::Literal`s at lowering time and substituted
//! directly into every reference -- see `lower_parameters`; a parameter
//! is not a signal and never appears in `ictus_ir::Module` at all),
//! constant and variable bit-select, constant part-select, concatenation
//! (plain `{a,b}` and replication/multiple concatenation `{N{a,b}}` --
//! see `lower_multiple_concatenation`, which needs no new IR: the count
//! must fold to a compile-time constant, same restriction a bit-select/
//! part-select bound already has, and the result is just the inner
//! concatenation's own parts physically repeated `N` times), and the
//! ternary operator on the *read* side (`x[3]`, `x[i]`, `x[7:0]`, `{a,b}`,
//! `{4{a,b}}`, `c ? a : b`; no indexed part-select `x[base +: width]`), plus
//! on the *assignment-target* side: a *constant* bit-select/part-select as
//! a non-blocking-assignment target (`x[7:0] <= v;`, picorv32's
//! `mem_rdata_q[...] <= ...` style -- see `lower_select_target_range`),
//! and a concatenation of such targets (`{a, b[3:0]} <= v;`, picorv32's
//! `{mem_rdata_q[31:25], mem_rdata_q[11:7]} <= ...` style -- see
//! `lower_concat_target_assign`, which splits it into one plain
//! `Stmt::NonBlockingAssign` per part rather than needing new IR; a
//! *nested* concatenation inside the target is rejected, not guessed at).
//! A variable index/indexed-range target, and any select or concatenation
//! as a *continuous*-assignment target, are still rejected, since only
//! `<=` has a commit phase to do the read-modify-write in -- see the
//! `NetLvalue::Lvalue` check in `lower_continuous_assign`. And expressions
//! built from literals (decimal/binary/hex; not octal, not X/Z-valued
//! outside a case item), signal references, unary `!`, the binary
//! operators `+ & | ^ == != < <= > >= && ||`, and the `$signed(...)`
//! system function (see `lower_system_function_call` and
//! `ictus_ir::Expr::Signed`'s doc comment -- v1 only implements this well
//! enough to sign-extend a value into a wider assignment target, which is
//! all picorv32 needs it for on the write side; using it as an operand of
//! `< <= > >=` is rejected rather than silently doing an unsigned
//! comparison, since a real signed *ordering* comparison isn't
//! implemented -- see `apply_binary_op`'s guard). Also lowers a
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
//! later phases need more of the language -- unary bitwise/reduction
//! operators (`~`, and the reduction forms `& | ^ ~& ~| ~^`/`^~`; only
//! logical `!` is lowered today -- see `lower_expr`'s `E::Unary` arm,
//! which is what picorv32 hits next, right after the replication-
//! concatenation support above), array/memory signals (`reg [31:0] mem
//! [0:31]`), `always_comb`, and module instantiation are the
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
            module.push_signal(lower_port(port, &tree, &mut last_direction)?);
        }
    }

    for decl_node in module_node.into_iter() {
        match decl_node {
            RefNode::NetDeclaration(sv_parser::NetDeclaration::NetType(net)) => {
                for signal in lower_internal_signal(&**net, &tree)? {
                    module.push_signal(signal);
                }
            }
            RefNode::DataDeclaration(DataDeclaration::Variable(var)) => {
                for signal in lower_internal_signal(&**var, &tree)? {
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

/// Parses the module's `#(parameter ...)` list into a name -> (value,
/// width) table. Parameters are never simulated as signals -- every
/// reference is fully resolved to an `Expr::Literal` right here at
/// lowering time, substituted directly into the expression tree (see
/// `lower_primary`), so `ictus_ir::Module` and the kernel never need to
/// know parameters exist. A parameter's default value uses Verilog's
/// separate *constant*-expression grammar (`ConstantParamExpression` ->
/// `ConstantExpression`, the same restricted grammar already used for
/// bit-select/part-select bounds -- see `lower_constant_index`), not the
/// general `Expression` this frontend's normal `lower_expr` handles, so
/// it's read directly here via the same "deep-search for the one Number"
/// approach rather than going through `lower_expr`/`Ctx` at all. That
/// grammar difference is also *why* a parameter default referencing
/// another parameter isn't supported: `ConstantPrimary`'s identifier
/// handling isn't wired up here since picorv32's own parameters never
/// cross-reference each other, so there was nothing to verify this
/// against yet -- a real gap if a future design needs it, not an
/// oversight to silently work around. Only a default that reduces to a
/// plain literal is supported; overriding a parameter at instantiation
/// (module instantiation isn't supported at all yet) is a separate,
/// larger gap.
fn lower_parameters(
    module_node: &sv_parser::ModuleDeclarationAnsi,
    tree: &SyntaxTree,
) -> Result<HashMap<String, (u64, u32)>, String> {
    let mut parameters = HashMap::new();

    for node in module_node.into_iter() {
        let RefNode::ParameterDeclarationParam(param_decl) = node else {
            continue;
        };

        let width = match unwrap_node!(&param_decl.nodes.1, PackedDimensionRange) {
            Some(range_node) => lower_packed_range(range_node, tree)?,
            None => 32,
        };

        for assignment in param_decl.nodes.2.nodes.0.contents() {
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
            let value = lower_constant_param_value(default, tree)
                .map_err(|e| format!("parameter '{name}' default: {e}"))?;

            parameters.insert(name, (value, width));
        }
    }

    Ok(parameters)
}

fn lower_constant_param_value(
    expr: &sv_parser::ConstantParamExpression,
    tree: &SyntaxTree,
) -> Result<u64, String> {
    let number_node =
        unwrap_node!(expr, Number).ok_or("is not a plain numeric literal in v1")?;
    let RefNode::Number(number) = number_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };
    match lower_number(number, tree)? {
        Expr::Literal { value, .. } => Ok(value),
        _ => unreachable!("lower_number always returns Expr::Literal"),
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

fn unsigned_number_str<'a>(node: RefNode<'a>, tree: &'a SyntaxTree) -> Option<&'a str> {
    match node {
        RefNode::UnsignedNumber(x) => tree.get_str(&x.nodes.0),
        _ => None,
    }
}

fn lower_port(
    port: &AnsiPortDeclaration,
    tree: &SyntaxTree,
    last_direction: &mut Option<Direction>,
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
        Some(range_node) => lower_packed_range(range_node, tree)?,
        None => 1,
    };

    Ok(Signal {
        name,
        width,
        direction: Some(direction),
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
fn lower_internal_signal<'a, T>(decl: &'a T, tree: &'a SyntaxTree) -> Result<Vec<Signal>, String>
where
    &'a T: IntoIterator<Item = RefNode<'a>>,
{
    let width = match unwrap_node!(decl, PackedDimensionRange) {
        Some(range_node) => lower_packed_range(range_node, tree)?,
        None => 1,
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

    Ok(names
        .into_iter()
        .map(|name| Signal {
            name,
            width,
            direction: None,
        })
        .collect())
}

fn lower_packed_range(range_node: RefNode, tree: &SyntaxTree) -> Result<u32, String> {
    let bounds: Vec<u32> = range_node
        .into_iter()
        .filter_map(|n| unsigned_number_str(n, tree))
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();
    match bounds.as_slice() {
        [msb, lsb] => Ok(msb.abs_diff(*lsb) + 1),
        _ => Err("packed range is not a simple `[N:M]` literal pair".to_string()),
    }
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
        StatementItem::SubroutineCallStatement(call) => lower_task_call_statement(call, tree, module),
        _ => Err(
            "statement form not supported in v1 (only begin/end blocks, if/else, case, \
             non-blocking assignment, and a call to a provably-empty task)"
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
    // A concatenation target (`{a, b} <= x;`) is its own `VariableLvalue`
    // variant (`Lvalue`, wrapping a brace-list of lvalues), not just an
    // identifier with an unusual `Select` -- checked separately, and
    // first, because the identifier-based checks below would otherwise
    // deep-search *past* this and silently match `a` alone, discarding
    // `b` and the split-assignment semantics entirely. Handled by
    // `lower_concat_target_assign`, which splits it into one
    // `Stmt::NonBlockingAssign` per part rather than needing new IR.
    if let sv_parser::VariableLvalue::Lvalue(concat) = &assign.nodes.0 {
        let value = lower_expr(&assign.nodes.3, tree, module)?;
        return lower_concat_target_assign(concat, value, tree, module);
    }

    let lhs_ident = unwrap_node!(&assign.nodes.0, SimpleIdentifier)
        .ok_or("non-blocking assignment target is not a simple identifier")?;
    let target_name = ident_str(lhs_ident, tree).ok_or("assignment target unreadable")?;
    let target = module
        .signal_id(target_name)
        .ok_or_else(|| format!("assignment target '{target_name}' is not a known signal"))?;
    let target_range =
        lower_select_target_range(&assign.nodes.0, target_name, tree, module)?;
    check_target_range(target_range, target, target_name, module)?;

    let value = lower_expr(&assign.nodes.3, tree, module)?;

    Ok(vec![Stmt::NonBlockingAssign {
        target,
        target_range,
        value,
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
        stmts.push(Stmt::NonBlockingAssign {
            target,
            target_range,
            value: Expr::Select {
                base: Box::new(value.clone()),
                msb,
                lsb,
            },
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
/// evaluation (see `ictus_kernel::eval_expr`) guarantees. Ordering
/// comparisons (`< <= > >=`) are different: real signed ordering needs a
/// genuinely different comparison (treating the sign-extended bit pattern
/// as a two's-complement negative number, not a huge positive one), which
/// this kernel doesn't implement -- so a `Signed` operand there is
/// rejected with a clear error instead of silently producing an *unsigned*
/// comparison result that happens to look plausible. (`*`, `>>>`, and
/// similar signed-sensitive operators aren't in this match at all yet --
/// see the `other` arm below -- so they're already safely rejected on
/// their own, before this function would ever need to reason about them.)
fn apply_binary_op(op_text: &str, lhs: Expr, rhs: Expr) -> Result<Expr, String> {
    let is_ordering_comparison = matches!(op_text, "<" | "<=" | ">" | ">=");
    if is_ordering_comparison && (matches!(lhs, Expr::Signed(..)) || matches!(rhs, Expr::Signed(..)))
    {
        return Err(
            "a signed comparison ($signed(...) as an operand of <, <=, >, or >=) is not \
             supported in v1 -- $signed(...) is only supported directly as (or within a \
             concatenation forming) an assignment's right-hand side, where sign extension \
             happens automatically at write time"
                .to_string(),
        );
    }

    match op_text {
        "+" => Ok(Expr::Add(Box::new(lhs), Box::new(rhs))),
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
/// supported yet. A single bit-select's index (`x[3]` or `x[i]`) can now
/// be anything: a constant literal lowers to `Expr::Select` as before,
/// anything else (a signal reference, arithmetic, ...) lowers to
/// `Expr::DynamicBitSelect` and is evaluated at simulation time.
fn lower_select(
    select: &sv_parser::Select,
    base: Expr,
    tree: &SyntaxTree,
    module: &Ctx,
) -> Result<Expr, String> {
    // Part-select: `x[msb:lsb]`.
    if let Some(bracket) = &select.nodes.2 {
        return match &bracket.nodes.1 {
            sv_parser::PartSelectRange::ConstantRange(range) => {
                let msb = lower_constant_index(&range.nodes.0, tree)?;
                let lsb = lower_constant_index(&range.nodes.2, tree)?;
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
        [only] => match lower_expr(&only.nodes.1, tree, module)? {
            Expr::Literal { value, .. } => {
                let bit = value as u32;
                Ok(Expr::Select {
                    base: Box::new(base),
                    msb: bit,
                    lsb: bit,
                })
            }
            index => Ok(Expr::DynamicBitSelect {
                base: Box::new(base),
                index: Box::new(index),
            }),
        },
        _ => Err("multi-dimensional array indexing is not supported in v1".to_string()),
    }
}

fn lower_constant_index(expr: &sv_parser::ConstantExpression, tree: &SyntaxTree) -> Result<u32, String> {
    let number_node = unwrap_node!(expr, Number)
        .ok_or("bit-select/part-select bound must be a plain numeric literal in v1")?;
    let RefNode::Number(number) = number_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };
    match lower_number(number, tree)? {
        Expr::Literal { value, .. } => Ok(value as u32),
        _ => unreachable!("lower_number always returns Expr::Literal"),
    }
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
        other => Err(format!(
            "concatenation operand's width can't be determined in v1 (only literals, signal \
             references, bit-select/part-select, nested concatenation, and ternary are \
             supported as operands): {other:?}"
        )),
    }
}

/// Extracts a `(msb, lsb)` write range from an assignment target that uses
/// a *constant* bit-select/part-select (`x[3:0] <= v;`, `x[5] <= v;`, or
/// `assign x[3:0] = v;`) -- `None` means the target is a plain reference
/// with no select at all, the overwhelmingly common case. `target` is
/// searched for a `Select` node the same way its identifier already is.
///
/// Only constant bounds are supported: a variable bit-select target
/// (`x[i] <= v;`) or an indexed part-select (`x[base +: width] <= v;`)
/// would need the kernel to compute the write range at simulation time
/// rather than lowering time, which the kernel doesn't implement -- both
/// are rejected here with a specific error rather than silently doing the
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
                let msb = lower_constant_index(&range.nodes.0, tree)?;
                let lsb = lower_constant_index(&range.nodes.2, tree)?;
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
        [only] => match lower_expr(&only.nodes.1, tree, module)? {
            Expr::Literal { value, .. } => {
                let bit = value as u32;
                Ok(Some((bit, bit)))
            }
            _ => Err(format!(
                "assignment target '{target_name}' uses a variable-indexed bit-select (`x[i] <= v;`), which v1 doesn't support as a write target"
            )),
        },
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
            let value = u64::from_str_radix(&strip_underscores(text), 2)
                .map_err(|_| format!("could not parse binary literal '{text}'"))?;
            Ok(Expr::Literal { value, width })
        }
        IntegralNumber::HexNumber(h) => {
            let width = lower_size(&h.nodes.0, tree)?;
            let text = locate_text(&h.nodes.2.nodes.0, tree)?;
            let value = u64::from_str_radix(&strip_underscores(text), 16)
                .map_err(|_| format!("could not parse hex literal '{text}'"))?;
            Ok(Expr::Literal { value, width })
        }
        IntegralNumber::OctalNumber(_) => Err("octal literals are not supported in v1".to_string()),
    }
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
        DecimalNumber::BaseXNumber(_) | DecimalNumber::BaseZNumber(_) => {
            Err("X/Z-valued literals are not supported (v1 is 2-state only)".to_string())
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
