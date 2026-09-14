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
//! constant and variable bit-select, constant part-select, concatenation,
//! and the ternary operator, all on the *read* side only (`x[3]`, `x[i]`,
//! `x[7:0]`, `{a,b}`, `c ? a : b`; no indexed part-select
//! `x[base +: width]`, no bit-select/part-select/concatenation as an
//! assignment target -- see `lower_select`/`lower_concatenation` and
//! `reject_select_target`/the `VariableLvalue::Lvalue`/`NetLvalue::Lvalue`
//! checks in the two assignment-lowering functions), and expressions
//! built from literals (decimal/binary/hex; not octal, not X/Z-valued
//! outside a case item), signal references, unary `!`, and the binary
//! operators `+ & | ^ == != < <= > >= && ||`. `lower_expr`'s `E::Binary`
//! arm also corrects a real `sv-parser` precedence-handling gap: it
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
//! later phases need more of the language -- bit-select/part-select as an
//! *assignment target* (confirmed needed: picorv32 does
//! `mem_rdata_q[...] <= ...`; needs read-modify-write semantics in the
//! kernel, not just frontend parsing, so it's a bigger step than the
//! read-side support that already exists), array/memory signals (`reg
//! [31:0] mem [0:31]`), `always_comb`, and module instantiation are the
//! next-highest-value gaps toward running a real design like phase 0's
//! picorv32 benchmark.

use ictus_ir::{Assign, CaseArm, CaseValue, ClockedProcess, Direction, Expr, Module, Signal, Stmt};
use std::collections::HashMap;
use std::path::Path;
use sv_parser::{
    parse_sv, unwrap_node, AlwaysConstruct, AnsiPortDeclaration, ConditionalStatement,
    ContinuousAssignNet, DataDeclaration, DecimalNumber, EdgeIdentifier, IntegralNumber, Locate,
    NonblockingAssignment, Number, PortDirection, RefNode, SeqBlock, StatementItem,
    StatementOrNull, SyntaxTree,
};

/// Bundles the module being lowered with its resolved parameter table,
/// threaded through expression lowering so `lower_primary` can resolve an
/// identifier against either. Derefs to `Module` so every existing
/// `module.signal_id(...)`/`module.signals[...]` call site throughout
/// this file keeps working unchanged -- only `lower_primary` needs
/// `.parameters` directly. Parameters themselves never appear in the
/// final `ictus_ir::Module`: each reference is fully resolved to an
/// `Expr::Literal` during lowering (see `lower_parameters`), so the IR
/// and the kernel never need to know parameters exist at all.
struct Ctx<'a> {
    module: &'a Module,
    parameters: &'a HashMap<String, (u64, u32)>,
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
            Ok(vec![lower_nonblocking_assign(assign, tree, module)?])
        }
        StatementItem::ConditionalStatement(cond) => Ok(vec![lower_if(cond, tree, module)?]),
        StatementItem::CaseStatement(case) => Ok(vec![lower_case(case, tree, module)?]),
        _ => Err(
            "statement form not supported in v1 (only begin/end blocks, if/else, case, and non-blocking assignment)"
                .to_string(),
        ),
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
) -> Result<Stmt, String> {
    // A concatenation target (`{a, b} <= x;`) is its own `VariableLvalue`
    // variant (`Lvalue`, wrapping a brace-list of lvalues), not just an
    // identifier with an unusual `Select` -- checked separately, and
    // first, because the identifier-based checks below would otherwise
    // deep-search *past* this and silently match `a` alone, discarding
    // `b` and the split-assignment semantics entirely.
    if let sv_parser::VariableLvalue::Lvalue(_) = &assign.nodes.0 {
        return Err(
            "assignment target is a concatenation (`{a, b} <= ...`), which v1 doesn't support as a write target"
                .to_string(),
        );
    }

    let lhs_ident = unwrap_node!(&assign.nodes.0, SimpleIdentifier)
        .ok_or("non-blocking assignment target is not a simple identifier")?;
    let target_name = ident_str(lhs_ident, tree).ok_or("assignment target unreadable")?;
    reject_select_target(&assign.nodes.0, target_name)?;
    let target = module
        .signal_id(target_name)
        .ok_or_else(|| format!("assignment target '{target_name}' is not a known signal"))?;

    let value = lower_expr(&assign.nodes.3, tree, module)?;

    Ok(Stmt::NonBlockingAssign { target, value })
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
    reject_select_target(&assignment.nodes.0, target_name)?;
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

fn apply_binary_op(op_text: &str, lhs: Expr, rhs: Expr) -> Result<Expr, String> {
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
        other => Err(format!("primary expression form not supported in v1: {other:?}")),
    }
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
        other => Err(format!(
            "concatenation operand's width can't be determined in v1 (only literals, signal \
             references, bit-select/part-select, nested concatenation, and ternary are \
             supported as operands): {other:?}"
        )),
    }
}

/// Rejects an assignment target that uses a bit-select/part-select
/// (`x[3:0] <= v;` or `assign x[3:0] = v;`) -- v1's `Signal` model has no
/// notion of a partial write, so lowering this as a full-width write to
/// `x` would silently discard the select and write the wrong bits rather
/// than fail loudly. `target` is searched for a `Select` node the same
/// way its identifier already is; a `Select` with no brackets (a plain
/// reference, the overwhelmingly common case) is fine and returns `Ok`.
fn reject_select_target<'a, T>(target: &'a T, target_name: &str) -> Result<(), String>
where
    &'a T: IntoIterator<Item = RefNode<'a>>,
{
    let Some(RefNode::Select(select)) = unwrap_node!(target, Select) else {
        return Ok(());
    };
    let has_select = !select.nodes.1.nodes.0.is_empty() || select.nodes.2.is_some();
    if has_select {
        return Err(format!(
            "assignment target '{target_name}' uses a bit-select/part-select, which v1 doesn't support as a write target (only as part of a read expression)"
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
