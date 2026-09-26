//! Verilog expression widths and signedness -- IEEE 1800 §11.6 and §11.8.1
//! -- made exact for a kernel that evaluates on 64-bit words.
//!
//! **The problem this solves.** The kernel evaluates every expression on a
//! `u64` and masks the result to its target's width when it is written.
//! That is right for operators whose low bits depend only on their
//! operands' low bits -- add, subtract, multiply, left shift, bitwise --
//! and wrong for anything that looks at *high* bits -- a right shift, a
//! comparison, equality, a truth test -- applied to an arithmetic result
//! that wrapped. Verilog evaluates `(a - b) >> 1` with 8-bit operands at 8
//! bits into an 8-bit target, so `3 - 5` becomes 254 *before* the shift;
//! the kernel used to shift `0xFFFF_FFFF_FFFF_FFFE`. Separately, `~a`
//! into a wider target inverts the *extended* operand (so the high bits
//! come out as ones), and a `$signed` operand mixed with an unsigned one
//! makes the whole expression unsigned. See docs/decisions.md D31.
//!
//! **The invariant.** After this pass, every expression evaluates to its
//! value in *canonical form*: an unsigned value of width W has every bit
//! at W and above clear; a signed value of width W is the full 64-bit
//! sign extension of its W-bit pattern. Given canonical operands, almost
//! every operator produces a canonical result by itself -- selects,
//! concatenations, comparisons, reductions, bitwise AND/OR/XOR, a logical
//! right shift of an unsigned value, an arithmetic right shift of a signed
//! one, a ternary. Only four can break it: add, subtract, multiply and
//! left shift (a carry or borrow escapes above bit W-1), plus bitwise NOT
//! of a signed value (the kernel's `BitwiseNot` masks). Those are the only
//! nodes this pass wraps. A canonical value of width W is also canonical
//! at every wider width -- zero- or sign-extension is already present --
//! so extending an operand to its context never needs a node of its own.
//!
//! **Where the width comes from.** Top-down, per §11.6: an assignment
//! evaluates its right-hand side at the wider of the target's width and
//! the expression's own; a context-determined operand (either side of
//! `+ - * & | ^`, the left of a shift, the operand of `~`, both arms of
//! `?:`) takes its parent's width; the two sides of a comparison take the
//! wider of the two, independent of what surrounds the comparison; and a
//! self-determined operand -- a shift amount, the operands of `&&`, `||`,
//! `!` and the reductions, a condition, an index, a concatenation part --
//! takes its own. A wrapped node is cut to the width it is *evaluated*
//! at, not its own: `(a - b) >> 1` into a 16-bit target subtracts at 16
//! bits, and gives 32767 for `3 - 5`, as Verilog does.
//!
//! **Not supported, deliberately:** an operator mixing a `$signed(...)`
//! operand with an unsigned one. Verilog makes such an expression
//! unsigned -- but an unsized decimal literal like `1` counts as *signed*,
//! and the lowered IR doesn't record which literals were unsized, so
//! `$signed(a) + 1` (signed in Verilog) and `$signed(a) + b` (unsigned)
//! can't be told apart here. Guessing either way is silently wrong for the
//! other, so both are rejected; `$signed(a) + $signed(b)` and `a + b` are
//! the explicit spellings.

use ictus_ir::{Expr, Module};

/// A Verilog expression's type as far as evaluation cares: its width and
/// whether it is signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExprType {
    pub width: u32,
    pub signed: bool,
}

/// Whether `expr` is a signed expression, per §11.8.1: `$signed(...)` is;
/// an operator whose operands are context-determined is signed only if
/// they all are; a shift takes its left operand's signedness; everything
/// else -- signals (Ictus has no `reg signed`), literals, selects,
/// concatenations, comparisons, reductions, logical operators -- is
/// unsigned. Needs no module, since nothing that depends on a signal's
/// declaration can be signed.
pub(crate) fn is_signed(expr: &Expr) -> bool {
    match expr {
        Expr::Signed(..) => true,
        Expr::BitwiseNot(inner, _) => is_signed(inner),
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::Xor(a, b) => is_signed(a) && is_signed(b),
        Expr::Shl(a, _) | Expr::Shr(a, _) | Expr::AShr(a, _) => is_signed(a),
        Expr::Ternary {
            then_val, else_val, ..
        } => is_signed(then_val) && is_signed(else_val),
        Expr::Literal { .. }
        | Expr::Ref(_)
        | Expr::Not(_)
        | Expr::ReduceAnd(..)
        | Expr::ReduceOr(..)
        | Expr::ReduceXor(..)
        | Expr::Eq(..)
        | Expr::Ne(..)
        | Expr::Lt(..)
        | Expr::Le(..)
        | Expr::Gt(..)
        | Expr::Ge(..)
        | Expr::SignedLt(..)
        | Expr::LogicalAnd(..)
        | Expr::LogicalOr(..)
        | Expr::Select { .. }
        | Expr::DynamicBitSelect { .. }
        | Expr::ArrayRead { .. }
        | Expr::Concat(_) => false,
    }
}

/// The type `expr` has on its own -- its *self-determined* width (§11.6.1,
/// Table 11-21) and its signedness. Rejects mixed signedness wherever
/// operands are context-determined together; see this module's comment.
pub(crate) fn self_type(expr: &Expr, module: &Module) -> Result<ExprType, String> {
    let unsigned = |width| Ok(ExprType {
        width,
        signed: false,
    });
    match expr {
        Expr::Literal { width, .. } => unsigned(*width),
        Expr::Ref(id) => unsigned(module.signals[*id].width),
        Expr::ArrayRead { array, .. } => unsigned(module.signals[*array].width),
        Expr::Select { msb, lsb, .. } => unsigned(msb - lsb + 1),
        Expr::DynamicBitSelect { .. } => unsigned(1),
        Expr::Concat(parts) => unsigned(parts.iter().map(|(_, w)| w).sum()),
        Expr::Not(_)
        | Expr::ReduceAnd(..)
        | Expr::ReduceOr(..)
        | Expr::ReduceXor(..)
        | Expr::LogicalAnd(..)
        | Expr::LogicalOr(..) => unsigned(1),
        Expr::Eq(a, b)
        | Expr::Ne(a, b)
        | Expr::Lt(a, b)
        | Expr::Le(a, b)
        | Expr::Gt(a, b)
        | Expr::Ge(a, b)
        | Expr::SignedLt(a, b) => {
            // The result is one bit, but the operands are sized and typed
            // against each other -- so they must agree on signedness.
            joint_type(&[a, b], module)?;
            unsigned(1)
        }
        Expr::BitwiseNot(inner, _) => self_type(inner, module),
        Expr::Signed(_, width) => Ok(ExprType {
            width: *width,
            signed: true,
        }),
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::Xor(a, b) => joint_type(&[a, b], module),
        // A shift is its left operand's width and type; the amount is
        // self-determined and plays no part.
        Expr::Shl(a, _) | Expr::Shr(a, _) | Expr::AShr(a, _) => self_type(a, module),
        Expr::Ternary {
            then_val, else_val, ..
        } => joint_type(&[then_val, else_val], module),
    }
}

/// The type of operands evaluated together: the widest width, signed only
/// if all are -- and an error if some are and some aren't.
fn joint_type(operands: &[&Expr], module: &Module) -> Result<ExprType, String> {
    let mut width = 0;
    let mut signed = Vec::with_capacity(operands.len());
    for operand in operands {
        let ty = self_type(operand, module)?;
        width = width.max(ty.width);
        signed.push(ty.signed);
    }
    let all = signed.iter().all(|&s| s);
    if !all && signed.iter().any(|&s| s) {
        return Err(
            "an operator mixes a `$signed(...)` operand with an unsigned one, which v1 \
             doesn't support: Verilog makes the whole expression unsigned, except that an \
             unsized decimal literal like `1` counts as signed -- and v1 can't tell which \
             literals were unsized, so `$signed(a) + 1` (signed) and `$signed(a) + b` \
             (unsigned) would look the same. Write both operands as `$signed(...)`, or \
             neither"
                .to_string(),
        );
    }
    Ok(ExprType { width, signed: all })
}

/// How much of a value its consumer reads.
///
/// Most operators compute their low W bits from their operands' low W bits
/// alone -- add, subtract, multiply, left shift, AND/OR/XOR, `~`, the arms
/// of `?:` -- so when a node's consumer only needs the low bits, so do its
/// operands, and nothing down that chain needs cutting to width. Canonical
/// form is only demanded where high bits are actually *read*: the operand
/// of a right shift, a comparison, a truth test, an index or shift amount,
/// a `case` selector. Tracking this is what keeps the wrappers off the
/// common case -- `x <= a + b` needs none at all, since the kernel masks
/// the written value to `x`'s width anyway -- which measurably matters on
/// picorv32, whose hot logic is full of additions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Demand {
    /// Every bit matters: the value must be in canonical form.
    Canonical,
    /// Only the low `ctx.width` bits are read.
    LowBits,
}

/// Prepares the right-hand side of an assignment to a target `target_width`
/// bits wide: evaluated at the wider of the target and itself (§11.6). Only
/// its low bits are demanded -- the kernel masks every written value to the
/// target's width, which is never wider than the evaluation width.
pub(crate) fn for_assignment(
    expr: Expr,
    target_width: u32,
    module: &Module,
) -> Result<Expr, String> {
    let ty = self_type(&expr, module)?;
    let ctx = ExprType {
        width: ty.width.max(target_width),
        signed: ty.signed,
    };
    contextualize(expr, ctx, Demand::LowBits, module)
}

/// Prepares an expression in a self-determined position -- a condition,
/// an index, a shift amount -- evaluated at its own width, and read whole.
pub(crate) fn self_determined(expr: Expr, module: &Module) -> Result<Expr, String> {
    let ty = self_type(&expr, module)?;
    contextualize(expr, ty, Demand::Canonical, module)
}

/// Prepares several expressions evaluated against each other -- a `case`
/// selector and its items -- at their joint width, which `extra_width` can
/// widen further (for items that aren't expressions, like `casez`
/// wildcards). Returns the prepared expressions and the width used.
pub(crate) fn jointly(
    exprs: Vec<Expr>,
    extra_width: u32,
    module: &Module,
) -> Result<(Vec<Expr>, u32), String> {
    let refs: Vec<&Expr> = exprs.iter().collect();
    let ty = joint_type(&refs, module)?;
    let ctx = ExprType {
        width: ty.width.max(extra_width),
        signed: ty.signed,
    };
    let prepared = exprs
        .into_iter()
        .map(|e| contextualize(e, ctx, Demand::Canonical, module))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((prepared, ctx.width))
}

/// Rewrites `expr`, evaluated in context `ctx`, so the kernel's plain
/// 64-bit evaluation yields Verilog's value -- in canonical form if
/// `demand` asks for it, or correct in its low `ctx.width` bits if that is
/// all its consumer reads. `ctx` is never narrower than `expr`'s own type:
/// every context is the maximum of something that includes it.
fn contextualize(expr: Expr, ctx: ExprType, demand: Demand, module: &Module) -> Result<Expr, String> {
    // A context-determined operand, at this node's context, with a given
    // demand; and a self-determined one, which is always read whole.
    let ctxz =
        |e: Box<Expr>, demand| contextualize(*e, ctx, demand, module).map(Box::new);
    let selfd = |e: Box<Expr>| self_determined(*e, module).map(Box::new);
    // Put a value that may carry bits past the context width into
    // canonical form -- but only if canonical form was asked for.
    let finish = |e: Expr| match demand {
        Demand::Canonical => canonical(e, ctx),
        Demand::LowBits => e,
    };

    Ok(match expr {
        // Already canonical: a stored signal, an element, an extracted
        // field, a 0/1 result. A literal is masked defensively, since a
        // width prefix narrower than its digits (`4'hFF`) would otherwise
        // carry bits above its width.
        Expr::Literal { value, width } => Expr::Literal {
            value: mask(value, width),
            width,
        },
        Expr::Ref(_) => expr,
        Expr::ArrayRead { array, index } => Expr::ArrayRead {
            array,
            index: selfd(index)?,
        },
        Expr::Select { base, msb, lsb } => Expr::Select {
            base: selfd(base)?,
            msb,
            lsb,
        },
        Expr::DynamicBitSelect { base, index } => Expr::DynamicBitSelect {
            base: selfd(base)?,
            index: selfd(index)?,
        },
        // Each part is self-determined, and the kernel masks it to its
        // own width as it packs it -- so only its low bits matter.
        Expr::Concat(parts) => Expr::Concat(
            parts
                .into_iter()
                .map(|(part, width)| {
                    let ty = self_type(&part, module)?;
                    Ok((contextualize(part, ty, Demand::LowBits, module)?, width))
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),

        // Self-determined operands, read whole; one-bit results.
        Expr::Not(a) => Expr::Not(selfd(a)?),
        Expr::LogicalAnd(a, b) => Expr::LogicalAnd(selfd(a)?, selfd(b)?),
        Expr::LogicalOr(a, b) => Expr::LogicalOr(selfd(a)?, selfd(b)?),
        Expr::ReduceAnd(a, w) => Expr::ReduceAnd(selfd(a)?, w),
        Expr::ReduceOr(a, w) => Expr::ReduceOr(selfd(a)?, w),
        Expr::ReduceXor(a, w) => Expr::ReduceXor(selfd(a)?, w),

        // Comparisons: the operands are sized to each other, whatever
        // surrounds the comparison, and read whole.
        Expr::Eq(a, b) => compare(Expr::Eq, a, b, module)?,
        Expr::Ne(a, b) => compare(Expr::Ne, a, b, module)?,
        Expr::Lt(a, b) => compare(Expr::Lt, a, b, module)?,
        Expr::Le(a, b) => compare(Expr::Le, a, b, module)?,
        Expr::Gt(a, b) => compare(Expr::Gt, a, b, module)?,
        Expr::Ge(a, b) => compare(Expr::Ge, a, b, module)?,
        Expr::SignedLt(a, b) => compare(Expr::SignedLt, a, b, module)?,

        // Low bits from low bits, and able to carry past the context
        // width: operands need only their low bits; the result is cut
        // back to width if its consumer reads the whole of it.
        Expr::Add(a, b) => finish(Expr::Add(ctxz(a, Demand::LowBits)?, ctxz(b, Demand::LowBits)?)),
        Expr::Sub(a, b) => finish(Expr::Sub(ctxz(a, Demand::LowBits)?, ctxz(b, Demand::LowBits)?)),
        Expr::Mul(a, b) => finish(Expr::Mul(ctxz(a, Demand::LowBits)?, ctxz(b, Demand::LowBits)?)),
        Expr::Shl(a, n) => finish(Expr::Shl(ctxz(a, Demand::LowBits)?, selfd(n)?)),

        // Low bits from low bits, and canonical from canonical: operands
        // get the same demand as the result.
        Expr::And(a, b) => Expr::And(ctxz(a, demand)?, ctxz(b, demand)?),
        Expr::Or(a, b) => Expr::Or(ctxz(a, demand)?, ctxz(b, demand)?),
        Expr::Xor(a, b) => Expr::Xor(ctxz(a, demand)?, ctxz(b, demand)?),
        Expr::Ternary {
            cond,
            then_val,
            else_val,
        } => Expr::Ternary {
            cond: selfd(cond)?,
            then_val: ctxz(then_val, demand)?,
            else_val: ctxz(else_val, demand)?,
        },

        // `~` inverts the operand *after* it is extended to the context,
        // so `~a` into a wider target has ones above `a`'s own width. The
        // kernel masks `BitwiseNot` to its width, so its operand only needs
        // low bits and an unsigned result is canonical as it stands.
        Expr::BitwiseNot(a, _) => {
            let inverted = Expr::BitwiseNot(ctxz(a, Demand::LowBits)?, ctx.width);
            if ctx.signed && demand == Demand::Canonical {
                // That mask strips a signed value's extension; restore it.
                Expr::Signed(Box::new(inverted), ctx.width)
            } else {
                inverted
            }
        }

        // A logical right shift reads the operand's high bits, so the
        // operand must be exactly its W-bit pattern. For a signed operand
        // that means dropping the extension first -- `>>` fills with zeros
        // whatever the operand's type.
        Expr::Shr(a, n) => {
            let operand = if ctx.signed {
                Box::new(truncate(*ctxz(a, Demand::LowBits)?, ctx.width))
            } else {
                ctxz(a, Demand::Canonical)?
            };
            let shifted = Expr::Shr(operand, selfd(n)?);
            if ctx.signed && demand == Demand::Canonical {
                Expr::Signed(Box::new(shifted), ctx.width)
            } else {
                shifted
            }
        }
        // An arithmetic right shift reads the sign extension, so its
        // operand must be canonical; the result then is too. The frontend
        // only builds `AShr` for a signed left operand -- `>>>` on an
        // unsigned one is a logical shift and lowers to `Shr`.
        Expr::AShr(a, n) => Expr::AShr(ctxz(a, Demand::Canonical)?, selfd(n)?),

        // `$signed(x)` reinterprets `x`'s bits, `x` being self-determined.
        // The kernel's `Signed` masks its operand to its width before
        // extending, so the operand only needs its low bits. In a signed
        // context this is sign extension; in an unsigned one it is plain
        // zero extension -- which only arises through mixing, rejected by
        // `self_type`, but the rule is Verilog's either way.
        Expr::Signed(a, width) => {
            let inner_ty = self_type(&a, module)?;
            let inner = contextualize(*a, inner_ty, Demand::LowBits, module)?;
            if ctx.signed {
                Expr::Signed(Box::new(inner), width)
            } else {
                truncate(inner, width)
            }
        }
    })
}

fn compare(
    build: fn(Box<Expr>, Box<Expr>) -> Expr,
    a: Box<Expr>,
    b: Box<Expr>,
    module: &Module,
) -> Result<Expr, String> {
    let ctx = joint_type(&[&a, &b], module)?;
    Ok(build(
        Box::new(contextualize(*a, ctx, Demand::Canonical, module)?),
        Box::new(contextualize(*b, ctx, Demand::Canonical, module)?),
    ))
}

/// Puts a value that may carry bits past `ctx.width` into canonical form.
fn canonical(expr: Expr, ctx: ExprType) -> Expr {
    if ctx.width >= 64 {
        expr
    } else if ctx.signed {
        // Masks to the width, then sign-extends from its top bit.
        Expr::Signed(Box::new(expr), ctx.width)
    } else {
        truncate(expr, ctx.width)
    }
}

/// Keeps only the low `width` bits.
fn truncate(expr: Expr, width: u32) -> Expr {
    if width >= 64 {
        expr
    } else {
        Expr::Select {
            base: Box::new(expr),
            msb: width - 1,
            lsb: 0,
        }
    }
}

fn mask(value: u64, width: u32) -> u64 {
    if width >= 64 {
        value
    } else {
        value & ((1u64 << width) - 1)
    }
}
