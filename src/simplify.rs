use crate::ast::Expr;
use crate::rational::{Rational, gcd_u128};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, BTreeSet};

/// Construct an exact reduced rational expression.
pub fn rat_expr(n: i128, d: i128) -> Result<Expr> {
    Ok(Expr::Rational(Rational::new(n, d)?))
}

/// Replace all free occurrences of `var` in `target` with `replacement`, then simplify.
pub fn subs(var: &str, replacement: &Expr, target: &Expr) -> Expr {
    let raw = subs_raw(var, replacement, target);
    simplify(&raw)
}

fn subs_raw(var: &str, replacement: &Expr, target: &Expr) -> Expr {
    match target {
        Expr::Symbol(s) if s == var => replacement.clone(),
        Expr::UnaryNeg(a) => Expr::UnaryNeg(Box::new(subs_raw(var, replacement, a))),
        Expr::Add(a, b) => Expr::Add(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::Sub(a, b) => Expr::Sub(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::Mul(a, b) => Expr::Mul(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::Div(a, b) => Expr::Div(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::Pow(a, b) => Expr::Pow(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::Call(name, args) => {
            if name == "__arrow__"
                && args.len() == 2
                && let Expr::List(params) = &args[0]
                && params
                    .iter()
                    .any(|p| matches!(p, Expr::Symbol(s) if s == var))
            {
                return target.clone();
            }
            Expr::Call(
                name.clone(),
                args.iter().map(|a| subs_raw(var, replacement, a)).collect(),
            )
        }
        Expr::Equation(a, b) => Expr::Equation(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::Range(a, b) => Expr::Range(
            Box::new(subs_raw(var, replacement, a)),
            Box::new(subs_raw(var, replacement, b)),
        ),
        Expr::List(items) => Expr::List(
            items
                .iter()
                .map(|a| subs_raw(var, replacement, a))
                .collect(),
        ),
        Expr::Matrix(rows) => Expr::Matrix(
            rows.iter()
                .map(|r| r.iter().map(|c| subs_raw(var, replacement, c)).collect())
                .collect(),
        ),
        _ => target.clone(),
    }
}

/// Evaluate an expression numerically to an IEEE-754 `f64`.
pub fn evalf(expr: &Expr) -> Result<f64> {
    match expr {
        Expr::Rational(r) => Ok(r.to_f64()),
        Expr::Float(v) => Ok(*v),
        Expr::Symbol(s) => match s.as_str() {
            "Pi" | "pi" => Ok(std::f64::consts::PI),
            "e" | "E" => Ok(std::f64::consts::E),
            "infinity" => Ok(f64::INFINITY),
            _ => bail!("cannot numerically evaluate free symbol `{s}`"),
        },
        Expr::UnaryNeg(a) => Ok(-evalf(a)?),
        Expr::Add(a, b) => Ok(evalf(a)? + evalf(b)?),
        Expr::Sub(a, b) => Ok(evalf(a)? - evalf(b)?),
        Expr::Mul(a, b) => Ok(evalf(a)? * evalf(b)?),
        Expr::Div(a, b) => Ok(evalf(a)? / evalf(b)?),
        Expr::Pow(a, b) => Ok(evalf(a)?.powf(evalf(b)?)),
        Expr::Call(name, args) => {
            if args.len() != 1 {
                bail!("function `{name}` requires 1 argument for evalf");
            }
            let x = evalf(&args[0])?;
            Ok(match name.as_str() {
                "sin" => x.sin(),
                "cos" => x.cos(),
                "tan" => x.tan(),
                "asin" | "arcsin" => x.asin(),
                "acos" | "arccos" => x.acos(),
                "atan" | "arctan" => x.atan(),
                "sinh" => x.sinh(),
                "cosh" => x.cosh(),
                "tanh" => x.tanh(),
                "exp" => x.exp(),
                "ln" | "log" => x.ln(),
                "log10" => x.log10(),
                "sqrt" => x.sqrt(),
                "abs" => x.abs(),
                "evalf" => x,
                _ => bail!("unsupported function `{name}` in evalf"),
            })
        }
        _ => bail!("cannot convert `{expr}` to floating-point value"),
    }
}

/// Collect all free variable names present in `expr`.
pub fn collect_symbols(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Symbol(s) => {
            if !matches!(s.as_str(), "Pi" | "pi" | "e" | "E" | "infinity") {
                out.insert(s.clone());
            }
        }
        Expr::UnaryNeg(a) => collect_symbols(a, out),
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::Div(a, b)
        | Expr::Pow(a, b)
        | Expr::Equation(a, b)
        | Expr::Range(a, b) => {
            collect_symbols(a, out);
            collect_symbols(b, out);
        }
        Expr::Call(_, args) | Expr::List(args) => {
            for a in args {
                collect_symbols(a, out);
            }
        }
        Expr::Matrix(rows) => {
            for r in rows {
                for c in r {
                    collect_symbols(c, out);
                }
            }
        }
        Expr::Rational(_) | Expr::Float(_) => {}
    }
}

/// Canonical algebraic simplifier.
pub fn simplify(expr: &Expr) -> Expr {
    match expr {
        Expr::Rational(r) => Expr::Rational(*r),
        Expr::Float(v) => Expr::Float(if *v == 0.0 { 0.0 } else { *v }),
        Expr::Symbol(s) => Expr::Symbol(s.clone()),
        Expr::UnaryNeg(inner) => simplify_neg(&simplify(inner)),
        Expr::Add(_, _) | Expr::Sub(_, _) => simplify_sum(expr),
        Expr::Mul(_, _) | Expr::Div(_, _) => simplify_product_or_div(expr),
        Expr::Pow(base, exp) => simplify_pow(&simplify(base), &simplify(exp)),
        Expr::Call(name, args) => simplify_call(name, args),
        Expr::Equation(lhs, rhs) => {
            Expr::Equation(Box::new(simplify(lhs)), Box::new(simplify(rhs)))
        }
        Expr::Range(lhs, rhs) => Expr::Range(Box::new(simplify(lhs)), Box::new(simplify(rhs))),
        Expr::List(items) => Expr::List(items.iter().map(simplify).collect()),
        Expr::Matrix(rows) => Expr::Matrix(
            rows.iter()
                .map(|r| r.iter().map(simplify).collect())
                .collect(),
        ),
    }
}

fn simplify_neg(expr: &Expr) -> Expr {
    match expr {
        Expr::Rational(r) => Expr::Rational(-*r),
        Expr::Float(v) => Expr::Float(if *v == 0.0 { 0.0 } else { -*v }),
        Expr::UnaryNeg(inner) => (**inner).clone(),
        Expr::Add(a, b) => simplify(&Expr::Add(
            Box::new(simplify_neg(a)),
            Box::new(simplify_neg(b)),
        )),
        Expr::Sub(a, b) => simplify(&Expr::Add(Box::new(simplify_neg(a)), b.clone())),
        Expr::Mul(lhs, rhs) => {
            if let Expr::Rational(r) = lhs.as_ref() {
                let nr = -*r;
                if nr.is_one() {
                    return (**rhs).clone();
                }
                return Expr::Mul(Box::new(Expr::Rational(nr)), rhs.clone());
            }
            if let Expr::Float(v) = lhs.as_ref() {
                return Expr::Mul(Box::new(Expr::Float(-*v)), rhs.clone());
            }
            Expr::Mul(
                Box::new(Expr::Rational(Rational::from_int(-1))),
                Box::new(expr.clone()),
            )
        }
        Expr::Div(num, den) => Expr::Div(Box::new(simplify_neg(num)), den.clone()),
        other => Expr::UnaryNeg(Box::new(other.clone())),
    }
}

fn simplify_call(name: &str, args: &[Expr]) -> Expr {
    if name == "__arrow__" && args.len() == 2 {
        return Expr::Call(name.to_string(), vec![args[0].clone(), simplify(&args[1])]);
    }
    let sargs: Vec<Expr> = args.iter().map(simplify).collect();
    if sargs.len() == 1 {
        let arg = &sargs[0];
        match name {
            "sin" => match arg {
                Expr::Rational(r) if r.is_zero() => return Expr::Rational(Rational::zero()),
                Expr::Symbol(s) if s == "Pi" => return Expr::Rational(Rational::zero()),
                Expr::Div(n, d)
                    if matches!(n.as_ref(), Expr::Symbol(s) if s == "Pi")
                        && matches!(d.as_ref(), Expr::Rational(r) if *r == Rational::from_int(2)) =>
                {
                    return Expr::Rational(Rational::one());
                }
                _ => {}
            },
            "cos" => match arg {
                Expr::Rational(r) if r.is_zero() => return Expr::Rational(Rational::one()),
                Expr::Symbol(s) if s == "Pi" => return Expr::Rational(Rational::from_int(-1)),
                Expr::Div(n, d)
                    if matches!(n.as_ref(), Expr::Symbol(s) if s == "Pi")
                        && matches!(d.as_ref(), Expr::Rational(r) if *r == Rational::from_int(2)) =>
                {
                    return Expr::Rational(Rational::zero());
                }
                _ => {}
            },
            "tan" => match arg {
                Expr::Rational(r) if r.is_zero() => return Expr::Rational(Rational::zero()),
                Expr::Symbol(s) if s == "Pi" => return Expr::Rational(Rational::zero()),
                _ => {}
            },
            "exp" => match arg {
                Expr::Rational(r) if r.is_zero() => return Expr::Rational(Rational::one()),
                Expr::Call(inner, iargs) if inner == "ln" && iargs.len() == 1 => {
                    return iargs[0].clone();
                }
                _ => {}
            },
            "ln" | "log" => match arg {
                Expr::Rational(r) if r.is_one() => return Expr::Rational(Rational::zero()),
                Expr::Symbol(s) if s == "e" => return Expr::Rational(Rational::one()),
                Expr::Call(inner, iargs) if inner == "exp" && iargs.len() == 1 => {
                    return iargs[0].clone();
                }
                _ => {}
            },
            "sqrt" => {
                if let Expr::Rational(r) = arg {
                    if let Some(exact) = exact_rational_sqrt(*r) {
                        return Expr::Rational(exact);
                    }
                    if r.is_positive()
                        && r.is_integer()
                        && r.num <= 1_000_000
                        && let Some((out_factor, inner)) = split_square_free(r.num as u128)
                        && out_factor > 1
                    {
                        return Expr::Mul(
                            Box::new(Expr::Rational(Rational::from_int(out_factor as i128))),
                            Box::new(Expr::Call(
                                "sqrt".to_string(),
                                vec![Expr::Rational(Rational::from_int(inner as i128))],
                            )),
                        );
                    }
                } else if let Expr::Float(v) = arg
                    && *v >= 0.0
                {
                    return Expr::Float(v.sqrt());
                }
            }
            "abs" => match arg {
                Expr::Rational(r) => return Expr::Rational(r.abs()),
                Expr::Float(v) => return Expr::Float(v.abs()),
                Expr::Symbol(s) if matches!(s.as_str(), "e" | "E" | "Pi" | "pi") => {
                    return Expr::Symbol(s.clone());
                }
                _ => {}
            },
            "factorial" => {
                if let Expr::Rational(r) = arg
                    && r.is_integer()
                    && !r.is_negative()
                    && r.num <= 20
                {
                    let mut acc: i128 = 1;
                    for k in 2..=r.num {
                        acc *= k;
                    }
                    return Expr::Rational(Rational::from_int(acc));
                }
            }
            _ => {}
        }
    }
    Expr::Call(name.to_string(), sargs)
}

fn exact_rational_sqrt(r: Rational) -> Option<Rational> {
    if r.is_negative() {
        return None;
    }
    if r.is_zero() {
        return Some(Rational::zero());
    }
    let sn = integer_sqrt(r.num as u128)?;
    let sd = integer_sqrt(r.den as u128)?;
    Rational::new(sn as i128, sd as i128).ok()
}

fn integer_sqrt(n: u128) -> Option<u128> {
    let root = (n as f64).sqrt() as u128;
    (root.saturating_sub(1)..=root.saturating_add(1))
        .find(|&cand| cand.checked_mul(cand) == Some(n))
}

fn split_square_free(mut n: u128) -> Option<(u128, u128)> {
    let mut outside = 1u128;
    let mut d = 2u128;
    while d * d <= n {
        while n.is_multiple_of(d * d) {
            outside *= d;
            n /= d * d;
        }
        d += if d == 2 { 1 } else { 2 };
    }
    Some((outside, n))
}

fn simplify_pow(base: &Expr, exp: &Expr) -> Expr {
    if matches!(exp, Expr::Rational(r) if r.is_zero()) {
        return Expr::Rational(Rational::one());
    }
    if matches!(exp, Expr::Rational(r) if r.is_one()) {
        return base.clone();
    }
    if matches!(base, Expr::Rational(r) if r.is_one()) {
        return Expr::Rational(Rational::one());
    }
    if matches!(base, Expr::Rational(r) if r.is_zero())
        && let Expr::Rational(e) = exp
        && e.is_positive()
    {
        return Expr::Rational(Rational::zero());
    }
    if let (Expr::Rational(b), Expr::Rational(e)) = (base, exp) {
        if e.is_integer()
            && e.num >= -32
            && e.num <= 32
            && let Ok(res) = b.pow(e.num as i32)
        {
            return Expr::Rational(res);
        }
        if *e == Rational::new(1, 2).unwrap() {
            return simplify_call("sqrt", &[Expr::Rational(*b)]);
        }
    }
    if let (Expr::Float(a), Expr::Float(b)) = (base, exp) {
        return Expr::Float(a.powf(*b));
    }
    if let (Expr::Float(a), Expr::Rational(b)) = (base, exp) {
        return Expr::Float(a.powf(b.to_f64()));
    }
    if let (Expr::Rational(a), Expr::Float(b)) = (base, exp) {
        return Expr::Float(a.to_f64().powf(*b));
    }
    // (u^m)^n -> u^(m*n) when m and n are integers or m is integer and n is integer.
    if let Expr::Pow(inner_base, inner_exp) = base
        && matches!(inner_exp.as_ref(), Expr::Rational(r) if r.is_integer())
        && matches!(exp, Expr::Rational(r) if r.is_integer())
    {
        let combined = simplify(&Expr::Mul(inner_exp.clone(), Box::new(exp.clone())));
        return simplify_pow(inner_base, &combined);
    }
    // sqrt(x)^2 -> x
    if let Expr::Call(name, args) = base
        && name == "sqrt"
        && args.len() == 1
        && matches!(exp, Expr::Rational(r) if *r == Rational::from_int(2))
    {
        return args[0].clone();
    }
    if let Expr::Rational(e) = exp
        && *e == Rational::new(1, 2).unwrap()
    {
        return Expr::Call("sqrt".to_string(), vec![base.clone()]);
    }
    if let Expr::Rational(e) = exp
        && e.is_negative()
        && e.is_integer()
    {
        let pos_exp = Expr::Rational(-*e);
        let denom = simplify_pow(base, &pos_exp);
        return simplify(&Expr::Div(
            Box::new(Expr::Rational(Rational::one())),
            Box::new(denom),
        ));
    }
    Expr::Pow(Box::new(base.clone()), Box::new(exp.clone()))
}

#[derive(Clone)]
struct Term {
    coeff_rat: Rational,
    coeff_float: Option<f64>,
    factors: Vec<(Expr, Expr)>, // (base, exponent)
}

impl Term {
    fn from_simplified_factor(expr: Expr) -> Self {
        match expr {
            Expr::Rational(r) => Self {
                coeff_rat: r,
                coeff_float: None,
                factors: Vec::new(),
            },
            Expr::Float(v) => Self {
                coeff_rat: Rational::one(),
                coeff_float: Some(v),
                factors: Vec::new(),
            },
            Expr::UnaryNeg(inner) => {
                let mut t = Self::from_simplified_factor(*inner);
                t.coeff_rat = -t.coeff_rat;
                t
            }
            Expr::Pow(base, exp) => Self {
                coeff_rat: Rational::one(),
                coeff_float: None,
                factors: vec![(*base, *exp)],
            },
            Expr::Mul(_, _) | Expr::Div(_, _) => {
                let mut factors = Vec::new();
                let mut coeff_rat = Rational::one();
                let mut coeff_float: Option<f64> = None;
                collect_mul_div(&expr, false, &mut coeff_rat, &mut coeff_float, &mut factors);
                Self {
                    coeff_rat,
                    coeff_float,
                    factors,
                }
            }
            other => Self {
                coeff_rat: Rational::one(),
                coeff_float: None,
                factors: vec![(other, Expr::Rational(Rational::one()))],
            },
        }
    }

    fn key(&self) -> String {
        let mut parts: Vec<String> = self
            .factors
            .iter()
            .map(|(b, e)| {
                if matches!(e, Expr::Rational(r) if r.is_one()) {
                    b.to_string()
                } else {
                    format!("{b}^{e}")
                }
            })
            .collect();
        parts.sort();
        parts.join("*")
    }

    fn degree_hint(&self) -> f64 {
        let mut deg = 0.0;
        for (b, e) in &self.factors {
            if matches!(b, Expr::Symbol(s) if !matches!(s.as_str(), "Pi" | "pi" | "e" | "E")) {
                if let Expr::Rational(r) = e {
                    deg += r.to_f64();
                } else {
                    deg += 1.0;
                }
            } else if matches!(b, Expr::Call(_, _)) {
                deg += 0.5;
            }
        }
        deg
    }
}

fn collect_mul_div(
    expr: &Expr,
    invert: bool,
    coeff_rat: &mut Rational,
    coeff_float: &mut Option<f64>,
    factors: &mut Vec<(Expr, Expr)>,
) {
    match expr {
        Expr::Mul(a, b) => {
            collect_mul_div(a, invert, coeff_rat, coeff_float, factors);
            collect_mul_div(b, invert, coeff_rat, coeff_float, factors);
        }
        Expr::Div(a, b) => {
            collect_mul_div(a, invert, coeff_rat, coeff_float, factors);
            collect_mul_div(b, !invert, coeff_rat, coeff_float, factors);
        }
        Expr::UnaryNeg(a) => {
            *coeff_rat = -*coeff_rat;
            collect_mul_div(a, invert, coeff_rat, coeff_float, factors);
        }
        Expr::Rational(r) => {
            if invert {
                if r.is_zero() {
                    // Preserve explicit zero divisor instead of panicking.
                    factors.push((Expr::Rational(*r), Expr::Rational(Rational::from_int(-1))));
                } else if let Ok(next) = coeff_rat.checked_div(*r) {
                    *coeff_rat = next;
                } else {
                    *coeff_float = Some(coeff_float.unwrap_or(1.0) / r.to_f64());
                }
            } else if let Ok(next) = coeff_rat.checked_mul(*r) {
                *coeff_rat = next;
            } else {
                *coeff_float = Some(coeff_float.unwrap_or(1.0) * r.to_f64());
            }
        }
        Expr::Float(v) => {
            let cur = coeff_float.unwrap_or(1.0);
            *coeff_float = Some(if invert { cur / *v } else { cur * *v });
        }
        Expr::Pow(base, exp) => {
            let adjusted = if invert {
                simplify_neg(exp)
            } else {
                (**exp).clone()
            };
            factors.push(((**base).clone(), adjusted));
        }
        other => {
            let exp = if invert {
                Expr::Rational(Rational::from_int(-1))
            } else {
                Expr::Rational(Rational::one())
            };
            factors.push((other.clone(), exp));
        }
    }
}

fn factor_sort_rank(base: &Expr) -> (u8, String) {
    match base {
        Expr::Symbol(s) if !matches!(s.as_str(), "Pi" | "pi" | "e" | "E") => (0, s.clone()),
        Expr::Symbol(s) => (1, s.clone()),
        Expr::Call(_, _) => (2, base.to_string()),
        _ => (3, base.to_string()),
    }
}

fn simplify_product_or_div(expr: &Expr) -> Expr {
    let s_expr = match expr {
        Expr::Mul(a, b) => Expr::Mul(Box::new(simplify(a)), Box::new(simplify(b))),
        Expr::Div(a, b) => Expr::Div(Box::new(simplify(a)), Box::new(simplify(b))),
        other => other.clone(),
    };

    let mut coeff_rat = Rational::one();
    let mut coeff_float: Option<f64> = None;
    let mut raw_factors = Vec::new();
    collect_mul_div(
        &s_expr,
        false,
        &mut coeff_rat,
        &mut coeff_float,
        &mut raw_factors,
    );

    let has_zero_denom = raw_factors.iter().any(|(b, e)| {
        matches!(b, Expr::Rational(r) if r.is_zero())
            && matches!(e, Expr::Rational(r) if r.is_negative())
    });

    if (coeff_rat.is_zero() || coeff_float == Some(0.0)) && !has_zero_denom {
        return if coeff_float.is_some() {
            Expr::Float(0.0)
        } else {
            Expr::Rational(Rational::zero())
        };
    }
    if has_zero_denom {
        return s_expr;
    }

    // Cancel identical numerator/denominator factors across `(x - 2)/(x - 2)` or `x^2 * x^3`.
    let mut grouped: Vec<(Expr, Vec<Expr>)> = Vec::new();
    for (base, exp) in raw_factors {
        if let Some((_, exps)) = grouped.iter_mut().find(|(b, _)| *b == base) {
            exps.push(exp);
        } else {
            grouped.push((base, vec![exp]));
        }
    }

    let mut num_factors: Vec<Expr> = Vec::new();
    let mut den_factors: Vec<Expr> = Vec::new();

    grouped.sort_by_key(|(a, _)| factor_sort_rank(a));

    for (base, exps) in grouped {
        let sum_exp = if exps.len() == 1 {
            exps.into_iter().next().unwrap()
        } else {
            let mut acc = exps[0].clone();
            for e in exps.into_iter().skip(1) {
                acc = Expr::Add(Box::new(acc), Box::new(e));
            }
            simplify(&acc)
        };

        if matches!(sum_exp, Expr::Rational(r) if r.is_zero()) {
            continue;
        }
        if let Expr::Rational(r) = sum_exp
            && r.is_negative()
        {
            let pos = Expr::Rational(-r);
            den_factors.push(simplify_pow(&base, &pos));
        } else if let Expr::UnaryNeg(inner) = sum_exp {
            den_factors.push(simplify_pow(&base, &inner));
        } else {
            num_factors.push(simplify_pow(&base, &sum_exp));
        }
    }

    // If numerator is 1 * sum and denominator is purely rational, or scalar multiple of single non-sum factor:
    if let Some(fv) = coeff_float {
        let combined = fv * coeff_rat.to_f64();
        let combined = if combined == 0.0 { 0.0 } else { combined };
        if num_factors.is_empty() && den_factors.is_empty() {
            return Expr::Float(combined);
        }
        return assemble_product_div(Rational::one(), Some(combined), num_factors, den_factors);
    }

    assemble_product_div(coeff_rat, None, num_factors, den_factors)
}

fn assemble_product_div(
    coeff_rat: Rational,
    coeff_float: Option<f64>,
    num_factors: Vec<Expr>,
    den_factors: Vec<Expr>,
) -> Expr {
    if coeff_rat.is_zero() {
        return Expr::Rational(Rational::zero());
    }
    let num_scalar = coeff_rat.num.abs();
    let den_scalar = coeff_rat.den;
    let is_neg = coeff_rat.num < 0;

    let mut num_expr = build_factor_chain(num_scalar, coeff_float, &num_factors);
    if is_neg {
        if let Some(ref mut e) = num_expr {
            if let Expr::Rational(r) = e {
                *e = Expr::Rational(-*r);
            } else {
                *e = match e {
                    Expr::Add(_, _) | Expr::Sub(_, _) => Expr::Mul(
                        Box::new(Expr::Rational(Rational::from_int(-1))),
                        Box::new(e.clone()),
                    ),
                    _ => Expr::Mul(
                        Box::new(Expr::Rational(Rational::from_int(-1))),
                        Box::new(e.clone()),
                    ),
                };
                // Normalize (-1)*expr to UnaryNeg when it is a standalone symbol/call/pow.
                if let Expr::Mul(l, r) = e
                    && matches!(l.as_ref(), Expr::Rational(x) if x.is_neg_one())
                    && !matches!(r.as_ref(), Expr::Add(_, _) | Expr::Sub(_, _))
                {
                    if let Expr::Mul(_, _) = r.as_ref() {
                        // Keep `(-1)*...` or fold `-1` into leading scalar if present.
                        if let Expr::Mul(rl, rr) = r.as_ref()
                            && let Expr::Rational(c) = rl.as_ref()
                        {
                            *e = Expr::Mul(Box::new(Expr::Rational(-*c)), rr.clone());
                        }
                    } else if den_factors.is_empty() && den_scalar == 1 {
                        *e = Expr::UnaryNeg(r.clone());
                    }
                }
            }
        } else {
            num_expr = Some(Expr::Rational(Rational::from_int(-1)));
        }
    }

    let mut num = num_expr.unwrap_or_else(|| Expr::Rational(Rational::one()));

    if den_factors.is_empty() && den_scalar == 1 {
        return num;
    }

    let den_list = den_factors;
    let den = build_factor_chain(den_scalar, None, &den_list)
        .unwrap_or_else(|| Expr::Rational(Rational::one()));

    // Fold rational numerator/denominator cleanly when there are no symbolic denominator factors.
    if den_list.is_empty()
        && let Expr::Rational(r) = num
    {
        return Expr::Rational(r / Rational::from_int(den_scalar));
    }
    if den_scalar > 1
        && num_scalar == 1
        && coeff_float.is_none()
        && !matches!(num, Expr::Rational(_))
    {
        let coeff = Rational::new(if is_neg { -1 } else { 1 }, den_scalar).unwrap();
        if let Expr::Mul(l, r) = &num
            && matches!(l.as_ref(), Expr::Rational(x) if x.is_neg_one())
        {
            return Expr::Mul(Box::new(Expr::Rational(coeff)), r.clone());
        }
        return Expr::Mul(Box::new(Expr::Rational(coeff)), Box::new(num));
    }

    // Represent `(c_num / c_den) * sym` as `(c_num/c_den)*sym` when denominator is purely numeric.
    if den_list.is_empty() && den_scalar > 1 && coeff_float.is_none() {
        let mut sym_factors = num_factors;
        let coeff =
            Rational::new(if is_neg { -num_scalar } else { num_scalar }, den_scalar).unwrap();
        if sym_factors.is_empty() {
            return Expr::Rational(coeff);
        }
        let mut chain = sym_factors.remove(0);
        for f in sym_factors {
            chain = Expr::Mul(Box::new(chain), Box::new(f));
        }
        return Expr::Mul(Box::new(Expr::Rational(coeff)), Box::new(chain));
    }

    if is_neg
        && let Expr::Mul(l, r) = &num
        && matches!(l.as_ref(), Expr::Rational(x) if x.is_neg_one())
    {
        num = Expr::UnaryNeg(r.clone());
    }

    Expr::Div(Box::new(num), Box::new(den))
}

fn build_factor_chain(scalar: i128, float_scalar: Option<f64>, factors: &[Expr]) -> Option<Expr> {
    let mut parts: Vec<Expr> = Vec::new();
    if let Some(fv) = float_scalar {
        let scaled = fv * (scalar as f64);
        if (scaled - 1.0).abs() > f64::EPSILON || factors.is_empty() {
            parts.push(Expr::Float(scaled));
        }
    } else if scalar != 1 || factors.is_empty() {
        parts.push(Expr::Rational(Rational::from_int(scalar)));
    }
    parts.extend(factors.iter().cloned());
    if parts.is_empty() {
        return None;
    }
    let mut iter = parts.into_iter();
    let mut acc = iter.next().unwrap();
    for p in iter {
        acc = Expr::Mul(Box::new(acc), Box::new(p));
    }
    Some(acc)
}

fn collect_sum_terms(expr: &Expr, negate: bool, out: &mut Vec<Expr>) {
    match expr {
        Expr::Add(a, b) => {
            collect_sum_terms(a, negate, out);
            collect_sum_terms(b, negate, out);
        }
        Expr::Sub(a, b) => {
            collect_sum_terms(a, negate, out);
            collect_sum_terms(b, !negate, out);
        }
        other => {
            let s = simplify(other);
            match s {
                Expr::Add(_, _) | Expr::Sub(_, _) => collect_sum_terms(&s, negate, out),
                _ => out.push(if negate { simplify_neg(&s) } else { s }),
            }
        }
    }
}

struct SumBucket {
    deg: f64,
    factors: Vec<(Expr, Expr)>,
    coeff_rat: Rational,
    coeff_float: Option<f64>,
}

fn simplify_sum(expr: &Expr) -> Expr {
    let mut flat = Vec::new();
    collect_sum_terms(expr, false, &mut flat);

    let mut const_rat = Rational::zero();
    let mut const_float: Option<f64> = None;
    let mut grouped: BTreeMap<String, SumBucket> = BTreeMap::new();

    for term_expr in flat {
        let term = Term::from_simplified_factor(term_expr);
        if term.factors.is_empty() {
            if let Some(fv) = term.coeff_float {
                let val = fv * term.coeff_rat.to_f64();
                const_float = Some(const_float.unwrap_or(0.0) + val);
            } else {
                const_rat = const_rat + term.coeff_rat;
            }
            continue;
        }
        let key = term.key();
        let deg = term.degree_hint();
        let entry = grouped.entry(key).or_insert(SumBucket {
            deg,
            factors: term.factors,
            coeff_rat: Rational::zero(),
            coeff_float: None,
        });
        if let Some(fv) = term.coeff_float {
            let val = fv * term.coeff_rat.to_f64();
            entry.coeff_float = Some(entry.coeff_float.unwrap_or(0.0) + val);
        } else {
            entry.coeff_rat = entry.coeff_rat + term.coeff_rat;
        }
    }

    let mut buckets: Vec<(f64, String, Expr)> = Vec::new();
    for (
        key,
        SumBucket {
            deg,
            factors,
            coeff_rat: crat,
            coeff_float: cfloat,
        },
    ) in grouped
    {
        if crat.is_zero() && cfloat.is_none() {
            continue;
        }
        let expr = if let Some(fv) = cfloat {
            let total = fv + crat.to_f64();
            if total == 0.0 {
                continue;
            }
            let term_factors: Vec<Expr> = factors
                .into_iter()
                .map(|(b, e)| simplify_pow(&b, &e))
                .collect();
            let neg = total < 0.0;
            let mag = total.abs();
            let mut body = build_factor_chain(1, Some(mag), &term_factors).unwrap();
            if neg {
                body = simplify_neg(&body);
            }
            body
        } else {
            let mut num_f = Vec::new();
            let mut den_f = Vec::new();
            for (b, e) in factors {
                if let Expr::Rational(r) = e
                    && r.is_negative()
                {
                    den_f.push(simplify_pow(&b, &Expr::Rational(-r)));
                } else {
                    num_f.push(simplify_pow(&b, &e));
                }
            }
            assemble_product_div(crat, None, num_f, den_f)
        };
        buckets.push((deg, key, expr));
    }

    // Sort highest variable degree first, then lexicographically by key.
    buckets.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    let mut terms: Vec<Expr> = buckets.into_iter().map(|(_, _, e)| e).collect();
    if let Some(fv) = const_float {
        let total = fv + const_rat.to_f64();
        if total != 0.0 || terms.is_empty() {
            terms.push(Expr::Float(if total == 0.0 { 0.0 } else { total }));
        }
    } else if !const_rat.is_zero() || terms.is_empty() {
        terms.push(Expr::Rational(const_rat));
    }

    let mut iter = terms.into_iter();
    let mut acc = iter.next().unwrap();
    for t in iter {
        acc = Expr::Add(Box::new(acc), Box::new(t));
    }
    acc
}

/// Expand products and positive integer powers of sums algebraically.
pub fn expand(expr: &Expr) -> Expr {
    let raw = expand_raw(expr);
    simplify(&raw)
}

fn expand_raw(expr: &Expr) -> Expr {
    match expr {
        Expr::UnaryNeg(a) => Expr::UnaryNeg(Box::new(expand_raw(a))),
        Expr::Add(a, b) => Expr::Add(Box::new(expand_raw(a)), Box::new(expand_raw(b))),
        Expr::Sub(a, b) => Expr::Sub(Box::new(expand_raw(a)), Box::new(expand_raw(b))),
        Expr::Mul(a, b) => distribute_mul(&expand_raw(a), &expand_raw(b)),
        Expr::Div(a, b) => {
            let ea = expand_raw(a);
            let eb = expand_raw(b);
            if !matches!(eb, Expr::Add(_, _) | Expr::Sub(_, _)) {
                distribute_div(&ea, &eb)
            } else {
                Expr::Div(Box::new(ea), Box::new(eb))
            }
        }
        Expr::Pow(base, exp) => {
            let eb = expand_raw(base);
            let ee = simplify(exp);
            if let Expr::Rational(r) = ee
                && r.is_integer()
                && !r.is_negative()
                && r.num <= 16
            {
                let mut acc = Expr::Rational(Rational::one());
                for _ in 0..r.num {
                    acc = distribute_mul(&acc, &eb);
                }
                return acc;
            }
            Expr::Pow(Box::new(eb), Box::new(ee))
        }
        Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(expand_raw).collect()),
        Expr::List(items) => Expr::List(items.iter().map(expand_raw).collect()),
        other => other.clone(),
    }
}

fn split_add_sub(expr: &Expr, out: &mut Vec<Expr>) {
    match expr {
        Expr::Add(a, b) => {
            split_add_sub(a, out);
            split_add_sub(b, out);
        }
        Expr::Sub(a, b) => {
            split_add_sub(a, out);
            let mut rhs = Vec::new();
            split_add_sub(b, &mut rhs);
            for r in rhs {
                out.push(Expr::UnaryNeg(Box::new(r)));
            }
        }
        other => out.push(other.clone()),
    }
}

fn distribute_mul(lhs: &Expr, rhs: &Expr) -> Expr {
    let mut l_terms = Vec::new();
    let mut r_terms = Vec::new();
    split_add_sub(lhs, &mut l_terms);
    split_add_sub(rhs, &mut r_terms);
    let mut sum_terms = Vec::new();
    for lt in &l_terms {
        for rt in &r_terms {
            sum_terms.push(Expr::Mul(Box::new(lt.clone()), Box::new(rt.clone())));
        }
    }
    let mut iter = sum_terms.into_iter();
    let mut acc = iter.next().unwrap();
    for t in iter {
        acc = Expr::Add(Box::new(acc), Box::new(t));
    }
    acc
}

fn distribute_div(lhs: &Expr, rhs: &Expr) -> Expr {
    let mut l_terms = Vec::new();
    split_add_sub(lhs, &mut l_terms);
    let mut parts = Vec::new();
    for lt in l_terms {
        parts.push(Expr::Div(Box::new(lt), Box::new(rhs.clone())));
    }
    let mut iter = parts.into_iter();
    let mut acc = iter.next().unwrap();
    for t in iter {
        acc = Expr::Add(Box::new(acc), Box::new(t));
    }
    acc
}

/// Extract univariate polynomial coefficients `[c_0, c_1, ..., c_d]` in `var` if `expr` is polynomial in `var`.
pub fn extract_poly(expr: &Expr, var: &str) -> Option<Vec<Rational>> {
    let expanded = expand(expr);
    let mut terms = Vec::new();
    collect_sum_terms(&expanded, false, &mut terms);
    let mut coeffs: BTreeMap<usize, Rational> = BTreeMap::new();

    for t in terms {
        let mut c = Rational::one();
        let mut deg: usize = 0;
        if !extract_poly_term(&t, var, &mut c, &mut deg) {
            return None;
        }
        let entry = coeffs.entry(deg).or_insert_with(Rational::zero);
        *entry = (*entry).checked_add(c).ok()?;
    }

    let max_deg = coeffs.keys().copied().max().unwrap_or(0);
    let mut out = vec![Rational::zero(); max_deg + 1];
    for (d, c) in coeffs {
        out[d] = c;
    }
    while out.len() > 1 && out.last().is_some_and(|c| c.is_zero()) {
        out.pop();
    }
    Some(out)
}

fn extract_poly_term(expr: &Expr, var: &str, coeff: &mut Rational, deg: &mut usize) -> bool {
    match expr {
        Expr::Rational(r) => {
            if let Ok(next) = coeff.checked_mul(*r) {
                *coeff = next;
                true
            } else {
                false
            }
        }
        Expr::Symbol(s) if s == var => {
            *deg += 1;
            true
        }
        Expr::UnaryNeg(inner) => {
            *coeff = -*coeff;
            extract_poly_term(inner, var, coeff, deg)
        }
        Expr::Mul(a, b) => {
            extract_poly_term(a, var, coeff, deg) && extract_poly_term(b, var, coeff, deg)
        }
        Expr::Div(a, b) => {
            if let Expr::Rational(r) = b.as_ref()
                && !r.is_zero()
                && extract_poly_term(a, var, coeff, deg)
                && let Ok(next) = coeff.checked_div(*r)
            {
                *coeff = next;
                return true;
            }
            false
        }
        Expr::Pow(base, exp) => {
            if let (Expr::Symbol(s), Expr::Rational(r)) = (base.as_ref(), exp.as_ref())
                && s == var
                && r.is_integer()
                && !r.is_negative()
                && r.num <= 64
            {
                *deg += r.num as usize;
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Factor univariate polynomials with rational coefficients via Rational Root Theorem.
pub fn factor(expr: &Expr) -> Expr {
    let expanded = expand(expr);
    let mut syms = BTreeSet::new();
    collect_symbols(&expanded, &mut syms);
    if syms.len() != 1 {
        return expanded;
    }
    let var = syms.into_iter().next().unwrap();
    let Some(poly) = extract_poly(&expanded, &var) else {
        return expanded;
    };
    if poly.len() <= 2 {
        return expanded;
    }
    factor_univariate_poly(&var, &poly)
}

fn lcm_i128(a: i128, b: i128) -> i128 {
    if a == 0 || b == 0 {
        return 1;
    }
    let g = gcd_u128(a.unsigned_abs(), b.unsigned_abs()) as i128;
    (a / g).abs() * b.abs()
}

fn integer_divisors(n: i128) -> Vec<i128> {
    let abs = n.unsigned_abs();
    if abs == 0 {
        return vec![1];
    }
    let mut small = Vec::new();
    let mut large = Vec::new();
    let mut d: u128 = 1;
    while d * d <= abs && small.len() < 128 {
        if abs.is_multiple_of(d) {
            small.push(d as i128);
            if d * d != abs {
                large.push((abs / d) as i128);
            }
        }
        d += 1;
    }
    small.extend(large.into_iter().rev());
    small
}

fn poly_eval_rat(poly: &[Rational], r: Rational) -> Option<Rational> {
    let mut acc = Rational::zero();
    for &c in poly.iter().rev() {
        acc = acc.checked_mul(r).ok()?.checked_add(c).ok()?;
    }
    Some(acc)
}

fn synthetic_divide(poly: &[Rational], root: Rational) -> Vec<Rational> {
    let n = poly.len();
    let mut q = vec![Rational::zero(); n - 1];
    let mut carry = poly[n - 1];
    q[n - 2] = carry;
    for i in (1..n - 1).rev() {
        carry = poly[i] + carry * root;
        q[i - 1] = carry;
    }
    q
}

fn poly_to_expr(var: &str, coeffs: &[Rational]) -> Expr {
    let mut terms = Vec::new();
    for (deg, &c) in coeffs.iter().enumerate().rev() {
        if c.is_zero() {
            continue;
        }
        let term = match deg {
            0 => Expr::Rational(c),
            1 => simplify(&Expr::Mul(
                Box::new(Expr::Rational(c)),
                Box::new(Expr::Symbol(var.to_string())),
            )),
            d => simplify(&Expr::Mul(
                Box::new(Expr::Rational(c)),
                Box::new(Expr::Pow(
                    Box::new(Expr::Symbol(var.to_string())),
                    Box::new(Expr::Rational(Rational::from_int(d as i128))),
                )),
            )),
        };
        terms.push(term);
    }
    if terms.is_empty() {
        return Expr::Rational(Rational::zero());
    }
    let mut iter = terms.into_iter();
    let mut acc = iter.next().unwrap();
    for t in iter {
        acc = Expr::Add(Box::new(acc), Box::new(t));
    }
    simplify(&acc)
}

fn factor_univariate_poly(var: &str, coeffs: &[Rational]) -> Expr {
    let mut common_den = 1i128;
    for c in coeffs {
        common_den = lcm_i128(common_den, c.den);
    }
    let mut int_coeffs: Vec<i128> = coeffs
        .iter()
        .map(|c| c.num * (common_den / c.den))
        .collect();
    let mut content_gcd: u128 = 0;
    for &c in &int_coeffs {
        content_gcd = gcd_u128(content_gcd, c.unsigned_abs());
    }
    if content_gcd == 0 {
        return Expr::Rational(Rational::zero());
    }
    let content = Rational::new(content_gcd as i128, common_den).unwrap();
    for c in &mut int_coeffs {
        *c /= content_gcd as i128;
    }

    let mut zero_deg = 0usize;
    while zero_deg < int_coeffs.len() && int_coeffs[zero_deg] == 0 {
        zero_deg += 1;
    }

    let mut rem: Vec<Rational> = int_coeffs[zero_deg..]
        .iter()
        .map(|&v| Rational::from_int(v))
        .collect();

    let mut factors: Vec<Expr> = Vec::new();
    if zero_deg == 1 {
        factors.push(Expr::Symbol(var.to_string()));
    } else if zero_deg > 1 {
        factors.push(Expr::Pow(
            Box::new(Expr::Symbol(var.to_string())),
            Box::new(Expr::Rational(Rational::from_int(zero_deg as i128))),
        ));
    }

    let mut lead_scale = content;
    loop {
        if rem.len() <= 1 {
            if rem.len() == 1 {
                lead_scale = lead_scale * rem[0];
            }
            break;
        }
        if rem.len() == 2 {
            // Linear remainder `a*x + b` -> keep clean `a*(x + b/a)` or `a*x + b`.
            let a = rem[1];
            let b = rem[0];
            if !a.is_one() && lead_scale.is_one() {
                factors.push(poly_to_expr(var, &rem));
            } else {
                lead_scale = lead_scale * a;
                let root = -(b / a);
                factors.push(simplify(&Expr::Sub(
                    Box::new(Expr::Symbol(var.to_string())),
                    Box::new(Expr::Rational(root)),
                )));
            }
            break;
        }

        let const_int = rem[0].num;
        let lead_int = rem[rem.len() - 1].num;
        let p_divs = integer_divisors(const_int);
        let q_divs = integer_divisors(lead_int);
        let mut candidates = Vec::new();
        for p in &p_divs {
            for q in &q_divs {
                if let Ok(pos) = Rational::new(*p, *q) {
                    candidates.push(pos);
                    candidates.push(-pos);
                }
            }
        }
        candidates.sort();
        candidates.dedup();

        let found = candidates
            .into_iter()
            .find(|&r| poly_eval_rat(&rem, r) == Some(Rational::zero()));
        if let Some(r) = found {
            let linear = simplify(&Expr::Sub(
                Box::new(Expr::Symbol(var.to_string())),
                Box::new(Expr::Rational(r)),
            ));
            factors.push(linear);
            rem = synthetic_divide(&rem, r);
        } else {
            let lead = rem[rem.len() - 1];
            if !lead.is_one() && lead_scale.is_one() && factors.is_empty() {
                factors.push(poly_to_expr(var, &rem));
            } else {
                lead_scale = lead_scale * lead;
                let monic: Vec<Rational> = rem.iter().map(|&c| c / lead).collect();
                factors.push(poly_to_expr(var, &monic));
            }
            break;
        }
    }

    let mut out_parts = Vec::new();
    if !lead_scale.is_one() {
        out_parts.push(Expr::Rational(lead_scale));
    }
    out_parts.extend(factors);
    if out_parts.is_empty() {
        return Expr::Rational(Rational::one());
    }
    let mut iter = out_parts.into_iter();
    let mut acc = iter.next().unwrap();
    for part in iter {
        acc = Expr::Mul(Box::new(acc), Box::new(part));
    }
    acc
}
