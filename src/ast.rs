use crate::rational::Rational;
use std::fmt;

/// Symbolic abstract syntax tree node.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Rational(Rational),
    Float(f64),
    Symbol(String),
    UnaryNeg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Equation(Box<Expr>, Box<Expr>),
    Range(Box<Expr>, Box<Expr>),
    List(Vec<Expr>),
    Matrix(Vec<Vec<Expr>>),
}

/// Statement terminator (`;` prints output, `:` suppresses display).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminator {
    Semicolon,
    Colon,
}

/// Top-level Maple worksheet statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Assignment {
        name: String,
        value: Expr,
        terminator: Terminator,
    },
    FunctionDef {
        name: String,
        params: Vec<String>,
        body: Expr,
        terminator: Terminator,
    },
    EvalExpr {
        expr: Expr,
        terminator: Terminator,
    },
    Restart {
        terminator: Terminator,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    Lowest = 0,
    AssignEq = 1,
    AddSub = 2,
    MulDiv = 3,
    Unary = 4,
    Pow = 5,
    Atom = 6,
}

fn format_float(v: f64) -> String {
    let clean = if v == 0.0 { 0.0 } else { v };
    format!("{clean}")
}

fn extract_negated(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::Rational(r) if r.is_negative() => Some(Expr::Rational(-*r)),
        Expr::Float(v) if *v < 0.0 => Some(Expr::Float(-*v)),
        Expr::UnaryNeg(inner) => Some((**inner).clone()),
        Expr::Mul(lhs, rhs) => {
            if let Expr::Rational(r) = lhs.as_ref()
                && r.is_negative()
            {
                let pos = -*r;
                if pos.is_one() {
                    return Some((**rhs).clone());
                }
                return Some(Expr::Mul(Box::new(Expr::Rational(pos)), rhs.clone()));
            }
            if let Expr::Float(v) = lhs.as_ref()
                && *v < 0.0
            {
                return Some(Expr::Mul(Box::new(Expr::Float(-*v)), rhs.clone()));
            }
            None
        }
        Expr::Div(lhs, rhs) => extract_negated(lhs).map(|p| Expr::Div(Box::new(p), rhs.clone())),
        _ => None,
    }
}

fn flatten_add<'a>(expr: &'a Expr, terms: &mut Vec<(bool, &'a Expr)>, negate: bool) {
    match expr {
        Expr::Add(a, b) => {
            flatten_add(a, terms, negate);
            flatten_add(b, terms, negate);
        }
        Expr::Sub(a, b) => {
            flatten_add(a, terms, negate);
            flatten_add(b, terms, !negate);
        }
        _ => terms.push((negate, expr)),
    }
}

fn fmt_expr(
    expr: &Expr,
    f: &mut fmt::Formatter<'_>,
    parent_prec: Prec,
    is_rhs: bool,
) -> fmt::Result {
    match expr {
        Expr::Rational(r) => {
            let needs_parens = parent_prec > Prec::Atom
                || (r.is_negative() && parent_prec >= Prec::Pow && !is_rhs)
                || (!r.is_integer() && (parent_prec >= Prec::Pow));
            if needs_parens {
                write!(f, "({r})")
            } else {
                write!(f, "{r}")
            }
        }
        Expr::Float(v) => {
            let s = format_float(*v);
            if *v < 0.0 && parent_prec >= Prec::Pow && !is_rhs {
                write!(f, "({s})")
            } else {
                write!(f, "{s}")
            }
        }
        Expr::Symbol(s) => write!(f, "{s}"),
        Expr::UnaryNeg(inner) => {
            let needs_parens = parent_prec > Prec::Unary || (parent_prec == Prec::Pow && !is_rhs);
            if needs_parens {
                write!(f, "(-")?;
                fmt_expr(inner, f, Prec::Unary, false)?;
                write!(f, ")")
            } else {
                write!(f, "-")?;
                fmt_expr(inner, f, Prec::Unary, false)
            }
        }
        Expr::Add(_, _) | Expr::Sub(_, _) => {
            let needs_parens =
                parent_prec > Prec::AddSub || (parent_prec == Prec::AddSub && is_rhs);
            if needs_parens {
                write!(f, "(")?;
            }
            let mut terms = Vec::new();
            flatten_add(expr, &mut terms, false);
            for (i, (neg, term)) in terms.into_iter().enumerate() {
                if i == 0 {
                    if neg {
                        write!(f, "-")?;
                        fmt_expr(term, f, Prec::Unary, false)?;
                    } else {
                        fmt_expr(term, f, Prec::AddSub, false)?;
                    }
                } else if !neg {
                    if let Some(pos) = extract_negated(term) {
                        write!(f, " - ")?;
                        fmt_expr(&pos, f, Prec::AddSub, true)?;
                    } else {
                        write!(f, " + ")?;
                        fmt_expr(term, f, Prec::AddSub, false)?;
                    }
                } else if let Some(pos) = extract_negated(term) {
                    write!(f, " + ")?;
                    fmt_expr(&pos, f, Prec::AddSub, false)?;
                } else {
                    write!(f, " - ")?;
                    fmt_expr(term, f, Prec::AddSub, true)?;
                }
            }
            if needs_parens {
                write!(f, ")")?;
            }
            Ok(())
        }
        Expr::Mul(lhs, rhs) => {
            let needs_parens = parent_prec > Prec::MulDiv;
            if needs_parens {
                write!(f, "(")?;
            }
            if let Expr::Rational(r) = lhs.as_ref()
                && r.is_neg_one()
            {
                write!(f, "-")?;
                fmt_expr(rhs, f, Prec::Unary, false)?;
            } else {
                fmt_expr(lhs, f, Prec::MulDiv, false)?;
                write!(f, "*")?;
                fmt_expr(rhs, f, Prec::MulDiv, true)?;
            }
            if needs_parens {
                write!(f, ")")?;
            }
            Ok(())
        }
        Expr::Div(lhs, rhs) => {
            let needs_parens = parent_prec > Prec::MulDiv;
            if needs_parens {
                write!(f, "(")?;
            }
            fmt_expr(lhs, f, Prec::MulDiv, false)?;
            write!(f, "/")?;
            let rhs_needs = matches!(
                rhs.as_ref(),
                Expr::Add(_, _)
                    | Expr::Sub(_, _)
                    | Expr::Mul(_, _)
                    | Expr::Div(_, _)
                    | Expr::UnaryNeg(_)
            ) || matches!(rhs.as_ref(), Expr::Rational(r) if !r.is_integer() || r.is_negative());
            if rhs_needs {
                write!(f, "(")?;
                fmt_expr(rhs, f, Prec::Lowest, false)?;
                write!(f, ")")?;
            } else {
                fmt_expr(rhs, f, Prec::Pow, true)?;
            }
            if needs_parens {
                write!(f, ")")?;
            }
            Ok(())
        }
        Expr::Pow(base, exp) => {
            let needs_parens = parent_prec > Prec::Pow || (parent_prec == Prec::Pow && !is_rhs);
            if needs_parens {
                write!(f, "(")?;
            }
            fmt_expr(base, f, Prec::Pow, false)?;
            write!(f, "^")?;
            let exp_needs = matches!(
                exp.as_ref(),
                Expr::Add(_, _)
                    | Expr::Sub(_, _)
                    | Expr::Mul(_, _)
                    | Expr::Div(_, _)
                    | Expr::UnaryNeg(_)
            ) || matches!(exp.as_ref(), Expr::Rational(r) if !r.is_integer() || r.is_negative());
            if exp_needs {
                write!(f, "(")?;
                fmt_expr(exp, f, Prec::Lowest, false)?;
                write!(f, ")")?;
            } else {
                fmt_expr(exp, f, Prec::Pow, true)?;
            }
            if needs_parens {
                write!(f, ")")?;
            }
            Ok(())
        }
        Expr::Call(name, args) => match name.as_str() {
            "__arrow__" if args.len() == 2 => {
                if let Expr::List(params) = &args[0] {
                    if params.len() == 1 {
                        fmt_expr(&params[0], f, Prec::Lowest, false)?;
                    } else {
                        write!(f, "(")?;
                        for (i, p) in params.iter().enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            fmt_expr(p, f, Prec::Lowest, false)?;
                        }
                        write!(f, ")")?;
                    }
                    write!(f, " -> ")?;
                    fmt_expr(&args[1], f, Prec::Lowest, false)
                } else {
                    write!(f, "{name}(")?;
                    fmt_args(args, f)?;
                    write!(f, ")")
                }
            }
            "$" if args.len() == 2 => {
                fmt_expr(&args[0], f, Prec::Atom, false)?;
                write!(f, "$")?;
                fmt_expr(&args[1], f, Prec::Atom, false)
            }
            "__index__" if !args.is_empty() => {
                fmt_expr(&args[0], f, Prec::Atom, false)?;
                write!(f, "[")?;
                fmt_args(&args[1..], f)?;
                write!(f, "]")
            }
            _ => {
                write!(f, "{name}(")?;
                fmt_args(args, f)?;
                write!(f, ")")
            }
        },
        Expr::Equation(lhs, rhs) => {
            let needs_parens = parent_prec > Prec::AssignEq;
            if needs_parens {
                write!(f, "(")?;
            }
            fmt_expr(lhs, f, Prec::AssignEq, false)?;
            write!(f, " = ")?;
            fmt_expr(rhs, f, Prec::AssignEq, true)?;
            if needs_parens {
                write!(f, ")")?;
            }
            Ok(())
        }
        Expr::Range(lhs, rhs) => {
            fmt_expr(lhs, f, Prec::AssignEq, false)?;
            write!(f, "..")?;
            fmt_expr(rhs, f, Prec::AssignEq, true)
        }
        Expr::List(items) => {
            write!(f, "[")?;
            fmt_args(items, f)?;
            write!(f, "]")
        }
        Expr::Matrix(rows) => {
            write!(f, "Matrix([")?;
            for (r_idx, row) in rows.iter().enumerate() {
                if r_idx > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "[")?;
                fmt_args(row, f)?;
                write!(f, "]")?;
            }
            write!(f, "])")
        }
    }
}

fn fmt_args(args: &[Expr], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        fmt_expr(arg, f, Prec::Lowest, false)?;
    }
    Ok(())
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_expr(self, f, Prec::Lowest, false)
    }
}

/// Format an expression in Maple-style 2D Unicode math notation (`f := x³ sin(x)`).
pub fn format_pretty_math(expr: &Expr, assigned_name: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(name) = assigned_name {
        out.push_str(name);
        out.push_str(" := ");
    }
    fmt_pretty(expr, &mut out, Prec::Lowest, false);
    out
}

/// Render an expression to LaTeX (`\frac{...}{...}`, `\sqrt{...}`, matrices).
pub fn to_latex(expr: &Expr, assigned_name: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(name) = assigned_name {
        out.push_str(name);
        out.push_str(" := ");
    }
    fmt_latex(expr, &mut out, Prec::Lowest, false);
    out
}

fn pretty_symbol(sym: &str) -> &str {
    match sym {
        "Pi" | "pi" => "π",
        "infinity" => "∞",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "theta" => "θ",
        "lambda" => "λ",
        "sigma" => "σ",
        "omega" => "ω",
        _ => sym,
    }
}

fn to_superscript_int(mut n: i128) -> Option<String> {
    if n.unsigned_abs() > 999_999 {
        return None;
    }
    let mut out = String::new();
    if n < 0 {
        out.push('⁻');
        n = -n;
    }
    for ch in n.to_string().chars() {
        let sup = match ch {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            _ => return None,
        };
        out.push(sup);
    }
    Some(out)
}

fn fmt_pretty(expr: &Expr, out: &mut String, parent_prec: Prec, is_rhs: bool) {
    use std::fmt::Write;
    match expr {
        Expr::Rational(r) => {
            let needs_parens = parent_prec > Prec::Atom
                || (r.is_negative() && parent_prec >= Prec::Pow && !is_rhs)
                || (!r.is_integer() && parent_prec >= Prec::Pow);
            if needs_parens {
                let _ = write!(out, "({r})");
            } else {
                let _ = write!(out, "{r}");
            }
        }
        Expr::Float(v) => {
            let s = format_float(*v);
            if *v < 0.0 && parent_prec >= Prec::Pow && !is_rhs {
                let _ = write!(out, "({s})");
            } else {
                out.push_str(&s);
            }
        }
        Expr::Symbol(s) => out.push_str(pretty_symbol(s)),
        Expr::UnaryNeg(inner) => {
            let needs_parens = parent_prec > Prec::Unary || (parent_prec == Prec::Pow && !is_rhs);
            if needs_parens {
                out.push_str("(-");
                fmt_pretty(inner, out, Prec::Unary, false);
                out.push(')');
            } else {
                out.push('-');
                fmt_pretty(inner, out, Prec::Unary, false);
            }
        }
        Expr::Add(_, _) | Expr::Sub(_, _) => {
            let needs_parens =
                parent_prec > Prec::AddSub || (parent_prec == Prec::AddSub && is_rhs);
            if needs_parens {
                out.push('(');
            }
            let mut terms = Vec::new();
            flatten_add(expr, &mut terms, false);
            for (i, (neg, term)) in terms.into_iter().enumerate() {
                if i == 0 {
                    if neg {
                        out.push('-');
                        fmt_pretty(term, out, Prec::Unary, false);
                    } else {
                        fmt_pretty(term, out, Prec::AddSub, false);
                    }
                } else if !neg {
                    if let Some(pos) = extract_negated(term) {
                        out.push_str(" - ");
                        fmt_pretty(&pos, out, Prec::AddSub, true);
                    } else {
                        out.push_str(" + ");
                        fmt_pretty(term, out, Prec::AddSub, false);
                    }
                } else if let Some(pos) = extract_negated(term) {
                    out.push_str(" + ");
                    fmt_pretty(&pos, out, Prec::AddSub, false);
                } else {
                    out.push_str(" - ");
                    fmt_pretty(term, out, Prec::AddSub, true);
                }
            }
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Mul(lhs, rhs) => {
            let needs_parens = parent_prec > Prec::MulDiv;
            if needs_parens {
                out.push('(');
            }
            if let Expr::Rational(r) = lhs.as_ref()
                && r.is_neg_one()
            {
                out.push('-');
                fmt_pretty(rhs, out, Prec::Unary, false);
            } else {
                fmt_pretty(lhs, out, Prec::MulDiv, false);
                if matches!(rhs.as_ref(), Expr::Rational(_) | Expr::Float(_)) {
                    out.push('·');
                } else {
                    out.push(' ');
                }
                fmt_pretty(rhs, out, Prec::MulDiv, true);
            }
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Div(lhs, rhs) => {
            let needs_parens = parent_prec > Prec::MulDiv;
            if needs_parens {
                out.push('(');
            }
            fmt_pretty(lhs, out, Prec::MulDiv, false);
            out.push('/');
            let rhs_needs = matches!(
                rhs.as_ref(),
                Expr::Add(_, _)
                    | Expr::Sub(_, _)
                    | Expr::Mul(_, _)
                    | Expr::Div(_, _)
                    | Expr::UnaryNeg(_)
            ) || matches!(rhs.as_ref(), Expr::Rational(r) if !r.is_integer() || r.is_negative());
            if rhs_needs {
                out.push('(');
                fmt_pretty(rhs, out, Prec::Lowest, false);
                out.push(')');
            } else {
                fmt_pretty(rhs, out, Prec::Pow, true);
            }
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Pow(base, exp) => {
            let needs_parens = parent_prec > Prec::Pow || (parent_prec == Prec::Pow && !is_rhs);
            if needs_parens {
                out.push('(');
            }
            fmt_pretty(base, out, Prec::Pow, false);
            if let Expr::Rational(r) = exp.as_ref()
                && r.is_integer()
                && let Some(sup) = to_superscript_int(r.num)
            {
                out.push_str(&sup);
            } else {
                out.push('^');
                let exp_needs = matches!(
                    exp.as_ref(),
                    Expr::Add(_, _)
                        | Expr::Sub(_, _)
                        | Expr::Mul(_, _)
                        | Expr::Div(_, _)
                        | Expr::UnaryNeg(_)
                ) || matches!(exp.as_ref(), Expr::Rational(r) if !r.is_integer() || r.is_negative());
                if exp_needs {
                    out.push('(');
                    fmt_pretty(exp, out, Prec::Lowest, false);
                    out.push(')');
                } else {
                    fmt_pretty(exp, out, Prec::Pow, true);
                }
            }
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Call(name, args) => match name.as_str() {
            "sqrt" if args.len() == 1 => {
                out.push_str("√(");
                fmt_pretty(&args[0], out, Prec::Lowest, false);
                out.push(')');
            }
            "__arrow__" if args.len() == 2 => {
                if let Expr::List(params) = &args[0] {
                    if params.len() == 1 {
                        fmt_pretty(&params[0], out, Prec::Lowest, false);
                    } else {
                        out.push('(');
                        for (i, p) in params.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            fmt_pretty(p, out, Prec::Lowest, false);
                        }
                        out.push(')');
                    }
                    out.push_str(" -> ");
                    fmt_pretty(&args[1], out, Prec::Lowest, false);
                } else {
                    let _ = write!(out, "{name}(");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        fmt_pretty(a, out, Prec::Lowest, false);
                    }
                    out.push(')');
                }
            }
            _ => {
                out.push_str(name);
                out.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    fmt_pretty(a, out, Prec::Lowest, false);
                }
                out.push(')');
            }
        },
        Expr::Equation(lhs, rhs) => {
            fmt_pretty(lhs, out, Prec::AssignEq, false);
            out.push_str(" = ");
            fmt_pretty(rhs, out, Prec::AssignEq, true);
        }
        Expr::Range(lhs, rhs) => {
            fmt_pretty(lhs, out, Prec::AssignEq, false);
            out.push_str("..");
            fmt_pretty(rhs, out, Prec::AssignEq, true);
        }
        Expr::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                fmt_pretty(item, out, Prec::Lowest, false);
            }
            out.push(']');
        }
        Expr::Matrix(rows) => {
            out.push('[');
            for (r_idx, row) in rows.iter().enumerate() {
                if r_idx > 0 {
                    out.push_str("; ");
                }
                for (c_idx, cell) in row.iter().enumerate() {
                    if c_idx > 0 {
                        out.push_str(", ");
                    }
                    fmt_pretty(cell, out, Prec::Lowest, false);
                }
            }
            out.push(']');
        }
    }
}

fn latex_symbol(sym: &str) -> &str {
    match sym {
        "Pi" | "pi" => r"\pi",
        "infinity" => r"\infty",
        "alpha" => r"\alpha",
        "beta" => r"\beta",
        "gamma" => r"\gamma",
        "theta" => r"\theta",
        "lambda" => r"\lambda",
        "sigma" => r"\sigma",
        "omega" => r"\omega",
        _ => sym,
    }
}

fn fmt_latex(expr: &Expr, out: &mut String, parent_prec: Prec, is_rhs: bool) {
    use std::fmt::Write;
    match expr {
        Expr::Rational(r) => {
            if r.is_integer() {
                if r.is_negative() && parent_prec >= Prec::Pow && !is_rhs {
                    let _ = write!(out, "({})", r.num);
                } else {
                    let _ = write!(out, "{}", r.num);
                }
            } else {
                let needs_parens = parent_prec >= Prec::Pow;
                if needs_parens {
                    out.push_str(r"\left(");
                }
                if r.is_negative() {
                    let _ = write!(out, r"-\frac{{{}}}{{{}}}", r.num.abs(), r.den);
                } else {
                    let _ = write!(out, r"\frac{{{}}}{{{}}}", r.num, r.den);
                }
                if needs_parens {
                    out.push_str(r"\right)");
                }
            }
        }
        Expr::Float(v) => {
            let s = format_float(*v);
            if *v < 0.0 && parent_prec >= Prec::Pow && !is_rhs {
                let _ = write!(out, "({s})");
            } else {
                out.push_str(&s);
            }
        }
        Expr::Symbol(s) => out.push_str(latex_symbol(s)),
        Expr::UnaryNeg(inner) => {
            let needs_parens = parent_prec > Prec::Unary || (parent_prec == Prec::Pow && !is_rhs);
            if needs_parens {
                out.push('(');
            }
            out.push('-');
            fmt_latex(inner, out, Prec::Unary, false);
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Add(_, _) | Expr::Sub(_, _) => {
            let needs_parens =
                parent_prec > Prec::AddSub || (parent_prec == Prec::AddSub && is_rhs);
            if needs_parens {
                out.push('(');
            }
            let mut terms = Vec::new();
            flatten_add(expr, &mut terms, false);
            for (i, (neg, term)) in terms.into_iter().enumerate() {
                if i == 0 {
                    if neg {
                        out.push('-');
                        fmt_latex(term, out, Prec::Unary, false);
                    } else {
                        fmt_latex(term, out, Prec::AddSub, false);
                    }
                } else if !neg {
                    if let Some(pos) = extract_negated(term) {
                        out.push_str(" - ");
                        fmt_latex(&pos, out, Prec::AddSub, true);
                    } else {
                        out.push_str(" + ");
                        fmt_latex(term, out, Prec::AddSub, false);
                    }
                } else if let Some(pos) = extract_negated(term) {
                    out.push_str(" + ");
                    fmt_latex(&pos, out, Prec::AddSub, false);
                } else {
                    out.push_str(" - ");
                    fmt_latex(term, out, Prec::AddSub, true);
                }
            }
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Mul(lhs, rhs) => {
            let needs_parens = parent_prec > Prec::MulDiv;
            if needs_parens {
                out.push('(');
            }
            if let Expr::Rational(r) = lhs.as_ref()
                && r.is_neg_one()
            {
                out.push('-');
                fmt_latex(rhs, out, Prec::Unary, false);
            } else {
                fmt_latex(lhs, out, Prec::MulDiv, false);
                if matches!(rhs.as_ref(), Expr::Rational(_) | Expr::Float(_)) {
                    out.push_str(r" \cdot ");
                } else {
                    out.push(' ');
                }
                fmt_latex(rhs, out, Prec::MulDiv, true);
            }
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Div(lhs, rhs) => {
            out.push_str(r"\frac{");
            fmt_latex(lhs, out, Prec::Lowest, false);
            out.push_str("}{");
            fmt_latex(rhs, out, Prec::Lowest, false);
            out.push('}');
        }
        Expr::Pow(base, exp) => {
            let needs_parens = parent_prec > Prec::Pow || (parent_prec == Prec::Pow && !is_rhs);
            if needs_parens {
                out.push('(');
            }
            fmt_latex(base, out, Prec::Pow, false);
            out.push_str("^{");
            fmt_latex(exp, out, Prec::Lowest, false);
            out.push('}');
            if needs_parens {
                out.push(')');
            }
        }
        Expr::Call(name, args) => match name.as_str() {
            "sqrt" if args.len() == 1 => {
                out.push_str(r"\sqrt{");
                fmt_latex(&args[0], out, Prec::Lowest, false);
                out.push('}');
            }
            "abs" if args.len() == 1 => {
                out.push_str(r"\left|");
                fmt_latex(&args[0], out, Prec::Lowest, false);
                out.push_str(r"\right|");
            }
            "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "ln"
            | "log" | "exp" => {
                let _ = write!(out, r"\{name}(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    fmt_latex(a, out, Prec::Lowest, false);
                }
                out.push(')');
            }
            "__arrow__" if args.len() == 2 => {
                fmt_latex(&args[0], out, Prec::Lowest, false);
                out.push_str(r" \mapsto ");
                fmt_latex(&args[1], out, Prec::Lowest, false);
            }
            _ => {
                out.push_str(name);
                out.push('(');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    fmt_latex(a, out, Prec::Lowest, false);
                }
                out.push(')');
            }
        },
        Expr::Equation(lhs, rhs) => {
            fmt_latex(lhs, out, Prec::AssignEq, false);
            out.push_str(" = ");
            fmt_latex(rhs, out, Prec::AssignEq, true);
        }
        Expr::Range(lhs, rhs) => {
            fmt_latex(lhs, out, Prec::AssignEq, false);
            out.push_str("..");
            fmt_latex(rhs, out, Prec::AssignEq, true);
        }
        Expr::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                fmt_latex(item, out, Prec::Lowest, false);
            }
            out.push(']');
        }
        Expr::Matrix(rows) => {
            out.push_str(r"\begin{bmatrix} ");
            for (r_idx, row) in rows.iter().enumerate() {
                if r_idx > 0 {
                    out.push_str(r" \\ ");
                }
                for (c_idx, cell) in row.iter().enumerate() {
                    if c_idx > 0 {
                        out.push_str(" & ");
                    }
                    fmt_latex(cell, out, Prec::Lowest, false);
                }
            }
            out.push_str(r" \end{bmatrix}");
        }
    }
}
