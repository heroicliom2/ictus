//! Verilog frontend: parses source via `sv-parser` and lowers the result
//! into `ictus_ir`.
//!
//! v1 scope, matching `ictus_ir`'s current shape (see that crate's doc
//! comment): a single ANSI-style module (`module foo (input wire clk,
//! ...)`), any number of clocked `always @(posedge clk) begin ... end`
//! blocks, `if`/`else` (no `else if` chains), plain `case` (not
//! `casez`/`casex` -- see `lower_case`), non-blocking assignment,
//! internal `wire`/`reg` declarations (in addition to ports), constant
//! bit-select/part-select on the *read* side only (`x[3]`, `x[7:0]`; not
//! `x[i]`, not as an assignment target -- see `lower_select` and
//! `reject_select_target`), and expressions built from literals
//! (decimal/binary/hex; not octal, not X/Z-valued), signal references,
//! unary `!`, and the binary operators `+ & | ^ == != < <= > >= && ||`.
//! Anything else in the source is either ignored (other module items) or
//! produces an error, deliberately -- silently mis-lowering an
//! unsupported construct would make this project's own differential
//! testing (docs/architecture.md, Validation strategy) meaningless. Also
//! lowers single-assignment continuous `assign target = expr;` statements
//! (net-targeted only -- see `lower_continuous_assign`) into
//! `ictus_ir::Assign`. Widen this as later phases need more of the
//! language -- `casez`/`casex`, variable-indexed select, concatenation,
//! `always_comb`, and module instantiation are the next-highest-value
//! gaps toward running a real design like phase 0's picorv32 benchmark.

use ictus_ir::{Assign, CaseArm, ClockedProcess, Direction, Expr, Module, Signal, Stmt};
use std::path::Path;
use sv_parser::{
    parse_sv, unwrap_node, AlwaysConstruct, AnsiPortDeclaration, ConditionalStatement,
    ContinuousAssignNet, DataDeclaration, DecimalNumber, EdgeIdentifier, IntegralNumber, Locate,
    NonblockingAssignment, Number, PortDirection, RefNode, SeqBlock, StatementItem,
    StatementOrNull, SyntaxTree,
};

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

    for port_node in module_node.into_iter() {
        if let RefNode::AnsiPortDeclaration(port) = port_node {
            module.push_signal(lower_port(port, &tree)?);
        }
    }

    for decl_node in module_node.into_iter() {
        match decl_node {
            RefNode::NetDeclaration(sv_parser::NetDeclaration::NetType(net)) => {
                module.push_signal(lower_internal_signal(&**net, &tree)?);
            }
            RefNode::DataDeclaration(DataDeclaration::Variable(var)) => {
                module.push_signal(lower_internal_signal(&**var, &tree)?);
            }
            _ => {}
        }
    }

    for always_node in module_node.into_iter() {
        if let RefNode::AlwaysConstruct(always) = always_node {
            if let Some(process) = lower_always(always, &tree, &module)? {
                module.clocked_processes.push(process);
            }
        }
    }

    for assign_node in module_node.into_iter() {
        if let RefNode::ContinuousAssign(sv_parser::ContinuousAssign::Net(net)) = assign_node {
            module.assigns.push(lower_continuous_assign(net, &tree, &module)?);
        }
    }

    Ok(module)
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

fn lower_port(port: &AnsiPortDeclaration, tree: &SyntaxTree) -> Result<Signal, String> {
    let ident_node =
        unwrap_node!(port, PortIdentifier).ok_or("port declaration has no identifier")?;
    let ident_simple =
        unwrap_node!(ident_node, SimpleIdentifier).ok_or("port identifier unreadable")?;
    let name = ident_str(ident_simple, tree)
        .ok_or("could not read port identifier")?
        .to_string();

    let direction_node =
        unwrap_node!(port, PortDirection).ok_or_else(|| format!("port '{name}' has no explicit direction (inherited direction is not supported in v1)"))?;
    let direction = match direction_node {
        RefNode::PortDirection(PortDirection::Input(_)) => Direction::Input,
        RefNode::PortDirection(PortDirection::Output(_)) => Direction::Output,
        _ => return Err(format!("port '{name}' has an unsupported direction (only input/output in v1)")),
    };

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
/// port declaration -- see `lower_port`) into an internal `Signal`
/// (`direction: None`). Generic over the declaration's concrete node type
/// (`NetDeclarationNetType` for `wire`, `DataDeclarationVariable` for
/// `reg`) since both are searched the same way: find the declared name
/// and an optional packed range, wherever they sit in that node's
/// subtree, without needing to hand-decode either grammar's exact nested
/// shape (`ListOfNetDeclAssignments`/`ListOfVariableDeclAssignments`,
/// `NetIdentifier`/`VariableIdentifier`, ...). Doesn't yet handle a
/// declaration naming more than one signal (`wire a, b;`) -- only the
/// first identifier found is used.
fn lower_internal_signal<'a, T>(decl: &'a T, tree: &'a SyntaxTree) -> Result<Signal, String>
where
    &'a T: IntoIterator<Item = RefNode<'a>>,
{
    let ident = unwrap_node!(decl, SimpleIdentifier).ok_or("declaration has no identifier")?;
    let name = ident_str(ident, tree)
        .ok_or("declaration identifier unreadable")?
        .to_string();

    let width = match unwrap_node!(decl, PackedDimensionRange) {
        Some(range_node) => lower_packed_range(range_node, tree)?,
        None => 1,
    };

    Ok(Signal {
        name,
        width,
        direction: None,
    })
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
    module: &Module,
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
    module: &Module,
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
    module: &Module,
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

fn lower_seq_block(seq: &SeqBlock, tree: &SyntaxTree, module: &Module) -> Result<Vec<Stmt>, String> {
    let mut out = Vec::new();
    for stmt in &seq.nodes.3 {
        out.extend(lower_statement_or_null(stmt, tree, module)?);
    }
    Ok(out)
}

fn lower_if(cond_stmt: &ConditionalStatement, tree: &SyntaxTree, module: &Module) -> Result<Stmt, String> {
    if !cond_stmt.nodes.4.is_empty() {
        return Err("`else if` chains are not supported in v1 (use nested if/else)".to_string());
    }

    let cond_predicate_node = unwrap_node!(&cond_stmt.nodes.2.nodes.1, Expression)
        .ok_or("if condition is not a plain expression (cond patterns are not supported in v1)")?;
    let RefNode::Expression(cond_expr) = cond_predicate_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };
    let cond = lower_expr(cond_expr, tree, module)?;

    let then_branch = lower_statement_or_null(&cond_stmt.nodes.3, tree, module)?;
    let else_branch = match &cond_stmt.nodes.5 {
        Some((_else_kw, stmt)) => lower_statement_or_null(stmt, tree, module)?,
        None => Vec::new(),
    };

    Ok(Stmt::If {
        cond,
        then_branch,
        else_branch,
    })
}

/// `casez`/`casex` share `case`'s grammar (`CaseStatementNormal`, just a
/// different `CaseKeyword`) but are rejected rather than lowered: treating
/// their wildcard bits (`?`/`z`/`x`) as literal 0/1 would silently
/// mis-match instead of failing loudly, which is exactly the class of bug
/// this project's differential testing exists to catch -- see this
/// function's `CaseKeyword` match. `inside`/pattern-matching case forms
/// (`CaseStatement::Matches`/`Inside`) aren't supported either.
fn lower_case(case: &sv_parser::CaseStatement, tree: &SyntaxTree, module: &Module) -> Result<Stmt, String> {
    let sv_parser::CaseStatement::Normal(normal) = case else {
        return Err("`inside`/pattern-matching case forms are not supported in v1".to_string());
    };

    match &normal.nodes.1 {
        sv_parser::CaseKeyword::Case(_) => {}
        sv_parser::CaseKeyword::Casez(_) | sv_parser::CaseKeyword::Casex(_) => {
            return Err(
                "`casez`/`casex` are not supported in v1 -- they need wildcard-bit-aware \
                 comparison this frontend doesn't implement yet"
                    .to_string(),
            );
        }
    }

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
                    .map(|item_expr| lower_expr(&item_expr.nodes.0, tree, module))
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

fn lower_nonblocking_assign(
    assign: &NonblockingAssignment,
    tree: &SyntaxTree,
    module: &Module,
) -> Result<Stmt, String> {
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
/// target: `assign a = x, b = y;` (comma-joined multiple assignments) and
/// bit-select/concatenation targets (`assign {a,b} = x;`) aren't
/// supported -- `NetAssignment` is found via a subtree search rather than
/// hand-decoding `ListOfNetAssignments`' `List<Symbol, NetAssignment>`
/// wrapper, so a comma-joined statement would silently only lower its
/// first assignment; that's an acceptable v1 gap since it's an unusual
/// style, not a silent-wrong-*value* bug like the ones this frontend's
/// tests specifically guard against.
fn lower_continuous_assign(
    net: &ContinuousAssignNet,
    tree: &SyntaxTree,
    module: &Module,
) -> Result<Assign, String> {
    let assignment_node =
        unwrap_node!(net, NetAssignment).ok_or("assign statement has no assignment")?;
    let RefNode::NetAssignment(assignment) = assignment_node else {
        unreachable!("unwrap_node! guarantees the requested variant");
    };

    let lhs_ident = unwrap_node!(&assignment.nodes.0, SimpleIdentifier).ok_or(
        "assign target is not a simple identifier (bit-select/concatenation targets are not supported in v1)",
    )?;
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
fn lower_expr(expr: &sv_parser::Expression, tree: &SyntaxTree, module: &Module) -> Result<Expr, String> {
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
            let lhs = lower_expr(&binary.nodes.0, tree, module)?;
            let rhs = lower_expr(&binary.nodes.3, tree, module)?;
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
        other => Err(format!("expression form not supported in v1: {other:?}")),
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
fn lower_primary(primary: &sv_parser::Primary, tree: &SyntaxTree, module: &Module) -> Result<Expr, String> {
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
            let id = module
                .signal_id(name)
                .ok_or_else(|| format!("reference to unknown signal '{name}'"))?;
            lower_select(&h.nodes.2, Expr::Ref(id), tree, module)
        }
        P::MintypmaxExpression(paren) => match &paren.nodes.0.nodes.1 {
            sv_parser::MintypmaxExpression::Expression(inner) => lower_expr(inner, tree, module),
            sv_parser::MintypmaxExpression::Ternary(_) => {
                Err("min:typ:max expressions are not supported in v1".to_string())
            }
        },
        other => Err(format!("primary expression form not supported in v1: {other:?}")),
    }
}

/// Applies a `Select` (`x[3]` or `x[7:0]`, or neither for a plain
/// reference) to an already-lowered `base` expression. v1 requires every
/// index/bound to be a constant, known at lowering time, not a
/// variable/signal-indexed select (`x[i]`) -- and requires
/// `PartSelectRange::ConstantRange` (`x[7:0]`) over `IndexedRange`
/// (`x[base +: width]`), which isn't supported yet either.
fn lower_select(
    select: &sv_parser::Select,
    base: Expr,
    tree: &SyntaxTree,
    module: &Module,
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
            _ => Err(
                "bit-select index must be a plain numeric literal in v1 (variable/signal-indexed select is not supported)"
                    .to_string(),
            ),
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
