//! Verilog frontend: parses source via `sv-parser` and lowers the result
//! into `ictus_ir`.
//!
//! v1 scope, matching `ictus_ir`'s current shape (see that crate's doc
//! comment): a single ANSI-style module (`module foo (input wire clk,
//! ...)`), one clocked `always @(posedge clk) begin ... end` block,
//! `if`/`else` (no `else if` chains), non-blocking assignment, and
//! expressions built from literals, signal references, unary `!`, and
//! binary `+`. Anything else in the source is either ignored (other
//! module items) or produces an error, deliberately -- silently
//! mis-lowering an unsupported construct would make this project's own
//! differential testing (docs/architecture.md, Validation strategy)
//! meaningless. Widen this as later phases need more of the language.

use ictus_ir::{ClockedProcess, Direction, Expr, Module, Signal, Stmt};
use std::path::Path;
use sv_parser::{
    parse_sv, unwrap_node, AlwaysConstruct, AnsiPortDeclaration, ConditionalStatement,
    EdgeIdentifier, Locate, NonblockingAssignment, PortDirection, RefNode, SeqBlock,
    StatementItem, StatementOrNull, SyntaxTree,
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

    for always_node in module_node.into_iter() {
        if let RefNode::AlwaysConstruct(always) = always_node {
            if let Some(process) = lower_always(always, &tree, &module)? {
                module.clocked_processes.push(process);
            }
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
        _ => Err(
            "statement form not supported in v1 (only begin/end blocks, if/else, and non-blocking assignment)"
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

fn lower_nonblocking_assign(
    assign: &NonblockingAssignment,
    tree: &SyntaxTree,
    module: &Module,
) -> Result<Stmt, String> {
    let lhs_ident = unwrap_node!(&assign.nodes.0, SimpleIdentifier)
        .ok_or("non-blocking assignment target is not a simple identifier")?;
    let target_name = ident_str(lhs_ident, tree).ok_or("assignment target unreadable")?;
    let target = module
        .signal_id(target_name)
        .ok_or_else(|| format!("assignment target '{target_name}' is not a known signal"))?;

    let value = lower_expr(&assign.nodes.3, tree, module)?;

    Ok(Stmt::NonBlockingAssign { target, value })
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
                other => Err(format!("unsupported binary operator '{other}'")),
            }
        }
        other => Err(format!("expression form not supported in v1: {other:?}")),
    }
}

fn lower_primary(primary: &sv_parser::Primary, tree: &SyntaxTree, module: &Module) -> Result<Expr, String> {
    if let Some(number) = unwrap_node!(primary, Number) {
        return lower_number(number, tree);
    }
    if let Some(ident) = unwrap_node!(primary, HierarchicalIdentifier) {
        let simple =
            unwrap_node!(ident, SimpleIdentifier).ok_or("identifier reference unreadable")?;
        let name = ident_str(simple, tree).ok_or("identifier reference unreadable")?;
        let id = module
            .signal_id(name)
            .ok_or_else(|| format!("reference to unknown signal '{name}'"))?;
        return Ok(Expr::Ref(id));
    }
    Err(format!("primary expression form not supported in v1: {primary:?}"))
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

fn lower_number(node: RefNode, tree: &SyntaxTree) -> Result<Expr, String> {
    // v1 supports plain unsized decimals (`0`, `7`) and sized decimals
    // (`8'd0`) -- the two forms this frontend's target designs actually
    // use. Binary/octal/hex literals are a documented gap. Sized must be
    // checked first: its digits are themselves an UnsignedNumber node, so
    // checking the unsized case first would match those digits but lose
    // the declared width.
    if let Some(sized) = unwrap_node!(node.clone(), DecimalNumberBaseUnsigned) {
        let digits_node = unwrap_node!(sized.clone(), UnsignedNumber)
            .ok_or("sized decimal literal has no digits")?;
        let digits =
            unsigned_number_str(digits_node, tree).ok_or("could not read literal digits")?;
        let value = digits
            .parse::<u64>()
            .map_err(|_| format!("could not parse decimal literal '{digits}'"))?;

        let width = match unwrap_node!(sized, NonZeroUnsignedNumber) {
            Some(RefNode::NonZeroUnsignedNumber(x)) => {
                let text = tree
                    .get_str(&x.nodes.0)
                    .ok_or("could not read literal size")?;
                text.parse::<u32>()
                    .map_err(|_| format!("could not parse literal size '{text}'"))?
            }
            _ => 32,
        };
        return Ok(Expr::Literal { value, width });
    }

    if let Some(digits_node) = unwrap_node!(node, UnsignedNumber) {
        let text =
            unsigned_number_str(digits_node, tree).ok_or("could not read literal digits")?;
        let value = text
            .parse::<u64>()
            .map_err(|_| format!("could not parse decimal literal '{text}'"))?;
        return Ok(Expr::Literal { value, width: 32 });
    }

    Err("literal is not a supported decimal form in v1".to_string())
}
