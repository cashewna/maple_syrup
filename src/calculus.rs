use crate::ast::{Expr, RelOp};
use crate::rational::Rational;
use crate::simplify::{evalf, expand, extract_poly, simplify, subs};
use anyhow::{Result, bail};

/// Check whether `expr` contains free occurrences of `var`.
pub fn depends_on(expr: &Expr, var: &str) -> bool {
    match expr {
        Expr::Symbol(s) => s == var,
        Expr::UnaryNeg(a) => depends_on(a, var),
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::Div(a, b)
        | Expr::Pow(a, b)
        | Expr::Equation(a, b)
        | Expr::Inequality(_, a, b)
        | Expr::Range(a, b) => depends_on(a, var) || depends_on(b, var),
        Expr::Call(_, args) | Expr::List(args) => args.iter().any(|a| depends_on(a, var)),
        Expr::Matrix(rows) => rows.iter().any(|r| r.iter().any(|c| depends_on(c, var))),
        Expr::Rational(_) | Expr::Float(_) => false,
    }
}

/// Differentiate `expr` with respect to `var` `order` times.
pub fn differentiate_n(expr: &Expr, var: &str, order: usize) -> Result<Expr> {
    let mut cur = expr.clone();
    for _ in 0..order {
        cur = differentiate(&cur, var)?;
    }
    Ok(cur)
}

/// Symbolically differentiate `expr` with respect to `var`.
pub fn differentiate(expr: &Expr, var: &str) -> Result<Expr> {
    let raw = diff_raw(expr, var)?;
    Ok(simplify(&raw))
}

fn diff_raw(expr: &Expr, var: &str) -> Result<Expr> {
    if !depends_on(expr, var) {
        return Ok(Expr::Rational(Rational::zero()));
    }
    match expr {
        Expr::Rational(_) | Expr::Float(_) => Ok(Expr::Rational(Rational::zero())),
        Expr::Symbol(s) => {
            if s == var {
                Ok(Expr::Rational(Rational::one()))
            } else {
                Ok(Expr::Rational(Rational::zero()))
            }
        }
        Expr::UnaryNeg(u) => Ok(Expr::UnaryNeg(Box::new(diff_raw(u, var)?))),
        Expr::Add(u, v) => Ok(Expr::Add(
            Box::new(diff_raw(u, var)?),
            Box::new(diff_raw(v, var)?),
        )),
        Expr::Sub(u, v) => Ok(Expr::Sub(
            Box::new(diff_raw(u, var)?),
            Box::new(diff_raw(v, var)?),
        )),
        Expr::Mul(u, v) => {
            let du = diff_raw(u, var)?;
            let dv = diff_raw(v, var)?;
            Ok(Expr::Add(
                Box::new(Expr::Mul(Box::new(du), v.clone())),
                Box::new(Expr::Mul(u.clone(), Box::new(dv))),
            ))
        }
        Expr::Div(u, v) => {
            if !depends_on(v, var) {
                return Ok(Expr::Div(Box::new(diff_raw(u, var)?), v.clone()));
            }
            let du = diff_raw(u, var)?;
            let dv = diff_raw(v, var)?;
            let num = Expr::Sub(
                Box::new(Expr::Mul(Box::new(du), v.clone())),
                Box::new(Expr::Mul(u.clone(), Box::new(dv))),
            );
            let den = Expr::Pow(v.clone(), Box::new(Expr::Rational(Rational::from_int(2))));
            Ok(Expr::Div(Box::new(num), Box::new(den)))
        }
        Expr::Pow(u, v) => {
            let u_dep = depends_on(u, var);
            let v_dep = depends_on(v, var);
            match (u_dep, v_dep) {
                (false, false) => Ok(Expr::Rational(Rational::zero())),
                (true, false) => {
                    let du = diff_raw(u, var)?;
                    let exp_minus_1 =
                        Expr::Sub(v.clone(), Box::new(Expr::Rational(Rational::one())));
                    let power = Expr::Pow(u.clone(), Box::new(exp_minus_1));
                    Ok(Expr::Mul(
                        Box::new(Expr::Mul(v.clone(), Box::new(power))),
                        Box::new(du),
                    ))
                }
                (false, true) => {
                    let dv = diff_raw(v, var)?;
                    if matches!(u.as_ref(), Expr::Symbol(s) if s == "e") {
                        Ok(Expr::Mul(Box::new(expr.clone()), Box::new(dv)))
                    } else {
                        let ln_u = Expr::Call("ln".to_string(), vec![(**u).clone()]);
                        Ok(Expr::Mul(
                            Box::new(Expr::Mul(Box::new(expr.clone()), Box::new(ln_u))),
                            Box::new(dv),
                        ))
                    }
                }
                (true, true) => {
                    let du = diff_raw(u, var)?;
                    let dv = diff_raw(v, var)?;
                    let ln_u = Expr::Call("ln".to_string(), vec![(**u).clone()]);
                    let term1 = Expr::Mul(Box::new(dv), Box::new(ln_u));
                    let term2 = Expr::Div(Box::new(Expr::Mul(v.clone(), Box::new(du))), u.clone());
                    Ok(Expr::Mul(
                        Box::new(expr.clone()),
                        Box::new(Expr::Add(Box::new(term1), Box::new(term2))),
                    ))
                }
            }
        }
        Expr::Call(name, args) => {
            if args.len() != 1 {
                bail!(
                    "cannot differentiate `{name}` with {} arguments",
                    args.len()
                );
            }
            let u = &args[0];
            let du = diff_raw(u, var)?;
            let deriv = match name.as_str() {
                "sin" => Expr::Call("cos".to_string(), vec![u.clone()]),
                "cos" => Expr::UnaryNeg(Box::new(Expr::Call("sin".to_string(), vec![u.clone()]))),
                "tan" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Pow(
                        Box::new(Expr::Call("cos".to_string(), vec![u.clone()])),
                        Box::new(Expr::Rational(Rational::from_int(2))),
                    )),
                ),
                "asin" | "arcsin" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Call(
                        "sqrt".to_string(),
                        vec![Expr::Sub(
                            Box::new(Expr::Rational(Rational::one())),
                            Box::new(Expr::Pow(
                                Box::new(u.clone()),
                                Box::new(Expr::Rational(Rational::from_int(2))),
                            )),
                        )],
                    )),
                ),
                "acos" | "arccos" => Expr::UnaryNeg(Box::new(Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Call(
                        "sqrt".to_string(),
                        vec![Expr::Sub(
                            Box::new(Expr::Rational(Rational::one())),
                            Box::new(Expr::Pow(
                                Box::new(u.clone()),
                                Box::new(Expr::Rational(Rational::from_int(2))),
                            )),
                        )],
                    )),
                ))),
                "atan" | "arctan" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Add(
                        Box::new(Expr::Rational(Rational::one())),
                        Box::new(Expr::Pow(
                            Box::new(u.clone()),
                            Box::new(Expr::Rational(Rational::from_int(2))),
                        )),
                    )),
                ),
                "sinh" => Expr::Call("cosh".to_string(), vec![u.clone()]),
                "cosh" => Expr::Call("sinh".to_string(), vec![u.clone()]),
                "tanh" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Pow(
                        Box::new(Expr::Call("cosh".to_string(), vec![u.clone()])),
                        Box::new(Expr::Rational(Rational::from_int(2))),
                    )),
                ),
                "exp" => Expr::Call("exp".to_string(), vec![u.clone()]),
                "ln" | "log" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(u.clone()),
                ),
                "sqrt" => Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(Expr::Mul(
                        Box::new(Expr::Rational(Rational::from_int(2))),
                        Box::new(Expr::Call("sqrt".to_string(), vec![u.clone()])),
                    )),
                ),
                "abs" => Expr::Div(
                    Box::new(u.clone()),
                    Box::new(Expr::Call("abs".to_string(), vec![u.clone()])),
                ),
                _ => bail!("cannot differentiate unknown function `{name}`"),
            };
            Ok(Expr::Mul(Box::new(deriv), Box::new(du)))
        }
        Expr::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(diff_raw(item, var)?);
            }
            Ok(Expr::List(out))
        }
        Expr::Matrix(rows) => {
            let mut out = Vec::with_capacity(rows.len());
            for row in rows {
                let mut erow = Vec::with_capacity(row.len());
                for cell in row {
                    erow.push(diff_raw(cell, var)?);
                }
                out.push(erow);
            }
            Ok(Expr::Matrix(out))
        }
        _ => bail!("cannot differentiate `{expr}` with respect to `{var}`"),
    }
}

/// Symbolically integrate `expr` with respect to `var` (up to an omitted constant of integration).
pub fn integrate(expr: &Expr, var: &str) -> Result<Expr> {
    let s = simplify(expr);
    if let Some(res) = try_integrate(&s, var) {
        return Ok(simplify(&res));
    }
    let exp = expand(&s);
    if exp != s
        && let Some(res) = try_integrate(&exp, var)
    {
        return Ok(simplify(&res));
    }
    bail!("no closed-form antiderivative found for `{expr}` with respect to `{var}`")
}

/// Evaluate a definite integral `int(expr, var = a..b)`.
pub fn integrate_definite(expr: &Expr, var: &str, a: &Expr, b: &Expr) -> Result<Expr> {
    let s = simplify(expr);
    if let Ok(antideriv) = integrate(&s, var) {
        let fb = subs(var, b, &antideriv);
        let fa = subs(var, a, &antideriv);
        return Ok(simplify(&Expr::Sub(Box::new(fb), Box::new(fa))));
    }

    // Numeric fallback via Composite Simpson's Rule when symbolic antiderivative is unavailable.
    let lo = evalf(a)?;
    let hi = evalf(b)?;
    let n = 1000usize;
    let h = (hi - lo) / (n as f64);
    let eval_at = |x_val: f64| -> Result<f64> {
        let point = Expr::Float(x_val);
        evalf(&subs(var, &point, &s))
    };
    let mut sum = eval_at(lo)? + eval_at(hi)?;
    for k in 1..n {
        let xk = lo + (k as f64) * h;
        let weight = if k % 2 == 0 { 2.0 } else { 4.0 };
        sum += weight * eval_at(xk)?;
    }
    let val = sum * h / 3.0;
    if !val.is_finite() {
        bail!("definite integral diverged or produced non-finite value");
    }
    Ok(Expr::Float(val))
}

fn try_integrate(expr: &Expr, var: &str) -> Option<Expr> {
    if !depends_on(expr, var) {
        return Some(Expr::Mul(
            Box::new(expr.clone()),
            Box::new(Expr::Symbol(var.to_string())),
        ));
    }
    match expr {
        Expr::UnaryNeg(u) => try_integrate(u, var).map(|r| Expr::UnaryNeg(Box::new(r))),
        Expr::Add(a, b) => Some(Expr::Add(
            Box::new(try_integrate(a, var)?),
            Box::new(try_integrate(b, var)?),
        )),
        Expr::Sub(a, b) => Some(Expr::Sub(
            Box::new(try_integrate(a, var)?),
            Box::new(try_integrate(b, var)?),
        )),
        Expr::Symbol(s) if s == var => Some(Expr::Div(
            Box::new(Expr::Pow(
                Box::new(Expr::Symbol(var.to_string())),
                Box::new(Expr::Rational(Rational::from_int(2))),
            )),
            Box::new(Expr::Rational(Rational::from_int(2))),
        )),
        Expr::Mul(_, _) | Expr::Div(_, _) => integrate_product_or_quotient(expr, var),
        Expr::Pow(base, exp) => integrate_pow(base, exp, var),
        Expr::Call(name, args) if args.len() == 1 => integrate_call(name, &args[0], var),
        _ => None,
    }
}

fn split_const_and_var_factors(expr: &Expr, var: &str) -> (Expr, Expr) {
    let mut const_parts = Vec::new();
    let mut var_parts = Vec::new();
    collect_split_mul(expr, false, var, &mut const_parts, &mut var_parts);
    let c = combine_mul_list(const_parts);
    let v = combine_mul_list(var_parts);
    (simplify(&c), simplify(&v))
}

fn collect_split_mul(
    expr: &Expr,
    invert: bool,
    var: &str,
    consts: &mut Vec<Expr>,
    vars: &mut Vec<Expr>,
) {
    match expr {
        Expr::Mul(a, b) => {
            collect_split_mul(a, invert, var, consts, vars);
            collect_split_mul(b, invert, var, consts, vars);
        }
        Expr::Div(a, b) => {
            collect_split_mul(a, invert, var, consts, vars);
            collect_split_mul(b, !invert, var, consts, vars);
        }
        other => {
            let item = if invert {
                Expr::Div(
                    Box::new(Expr::Rational(Rational::one())),
                    Box::new(other.clone()),
                )
            } else {
                other.clone()
            };
            if depends_on(other, var) {
                vars.push(item);
            } else {
                consts.push(item);
            }
        }
    }
}

fn combine_mul_list(items: Vec<Expr>) -> Expr {
    if items.is_empty() {
        return Expr::Rational(Rational::one());
    }
    let mut iter = items.into_iter();
    let mut acc = iter.next().unwrap();
    for item in iter {
        acc = Expr::Mul(Box::new(acc), Box::new(item));
    }
    acc
}

fn integrate_product_or_quotient(expr: &Expr, var: &str) -> Option<Expr> {
    let (c, v) = split_const_and_var_factors(expr, var);
    if !c.eq(&Expr::Rational(Rational::one())) {
        let inner = try_integrate(&v, var)?;
        return Some(Expr::Mul(Box::new(c), Box::new(inner)));
    }

    // 1 / (...) patterns
    if let Expr::Div(num, den) = expr
        && !depends_on(num, var)
    {
        if let Some((a, _b)) = extract_linear(den, var) {
            let ln_den = Expr::Call("ln".to_string(), vec![(**den).clone()]);
            return Some(
                Expr::Div(
                    Box::new((**num).clone()),
                    Box::new(Expr::Mul(
                        Box::new(a),
                        Box::new(Expr::Div(
                            Box::new(Expr::Rational(Rational::one())),
                            Box::new(ln_den),
                        )),
                    )),
                )
                .replace_div_by_mul_recip(),
            );
        }
        // Check `1 / (a*x^2 + b)` -> `atan`
        if let Some(poly) = extract_poly(den, var)
            && poly.len() == 3
            && poly[1].is_zero()
            && poly[0].is_positive()
            && poly[2].is_positive()
        {
            // int(1 / (c2*x^2 + c0)) = 1/sqrt(c0*c2) * atan(sqrt(c2/c0)*x)
            let c0 = Expr::Rational(poly[0]);
            let c2 = Expr::Rational(poly[2]);
            let scale = Expr::Call(
                "sqrt".to_string(),
                vec![Expr::Mul(Box::new(c0.clone()), Box::new(c2.clone()))],
            );
            let ratio = Expr::Call(
                "sqrt".to_string(),
                vec![Expr::Div(Box::new(c2), Box::new(c0))],
            );
            let arg = Expr::Mul(Box::new(ratio), Box::new(Expr::Symbol(var.to_string())));
            return Some(Expr::Div(
                Box::new(Expr::Mul(
                    Box::new((**num).clone()),
                    Box::new(Expr::Call("atan".to_string(), vec![arg])),
                )),
                Box::new(scale),
            ));
        }
        // 1 / (a*x + b)^n
        if let Expr::Pow(base, exp) = den.as_ref()
            && !depends_on(exp, var)
        {
            let neg_exp = simplify(&Expr::UnaryNeg(exp.clone()));
            if let Some(res) = integrate_pow(base, &neg_exp, var) {
                return Some(Expr::Mul(Box::new((**num).clone()), Box::new(res)));
            }
        }
        // 1 / sqrt(1 - x^2)
        if let Expr::Call(fn_name, args) = den.as_ref()
            && fn_name == "sqrt"
            && args.len() == 1
            && let Some(poly) = extract_poly(&args[0], var)
            && poly.len() == 3
            && poly[0].is_one()
            && poly[1].is_zero()
            && poly[2] == Rational::from_int(-1)
        {
            return Some(Expr::Mul(
                Box::new((**num).clone()),
                Box::new(Expr::Call(
                    "asin".to_string(),
                    vec![Expr::Symbol(var.to_string())],
                )),
            ));
        }
    }

    // Linear-in-denominator shortcut already handled cleanly below without helper trait:
    if let Expr::Div(num, den) = expr
        && !depends_on(num, var)
        && let Some((a, _b)) = extract_linear(den, var)
    {
        let ln_den = Expr::Call("ln".to_string(), vec![(**den).clone()]);
        return Some(Expr::Div(
            Box::new(Expr::Mul(Box::new((**num).clone()), Box::new(ln_den))),
            Box::new(a),
        ));
    }

    // Attempt u-substitution on `g'(x)/g(x)` or `f(g(x)) * g'(x)`.
    if let Some(u_sub) = try_u_substitution(expr, var) {
        return Some(u_sub);
    }

    // Attempt tabular integration by parts for `poly(x) * {exp, sin, cos}(a*x + b)`.
    if let Some(ibp) = try_tabular_ibp(expr, var) {
        return Some(ibp);
    }

    None
}

trait DivFixup {
    fn replace_div_by_mul_recip(self) -> Self;
}

impl DivFixup for Expr {
    fn replace_div_by_mul_recip(self) -> Self {
        self
    }
}

fn integrate_pow(base: &Expr, exp: &Expr, var: &str) -> Option<Expr> {
    if !depends_on(base, var) {
        // c^u where u is linear in var -> c^(a*x+b) / (a * ln(c))
        let (a, _b) = extract_linear(exp, var)?;
        if matches!(base, Expr::Symbol(s) if s == "e") {
            return Some(Expr::Div(
                Box::new(Expr::Pow(Box::new(base.clone()), Box::new(exp.clone()))),
                Box::new(a),
            ));
        }
        let ln_base = Expr::Call("ln".to_string(), vec![base.clone()]);
        return Some(Expr::Div(
            Box::new(Expr::Pow(Box::new(base.clone()), Box::new(exp.clone()))),
            Box::new(Expr::Mul(Box::new(a), Box::new(ln_base))),
        ));
    }

    if !depends_on(exp, var)
        && let Some((a, _b)) = extract_linear(base, var)
    {
        if matches!(exp, Expr::Rational(r) if *r == Rational::from_int(-1)) {
            let ln_base = Expr::Call("ln".to_string(), vec![base.clone()]);
            return Some(Expr::Div(Box::new(ln_base), Box::new(a)));
        }
        let new_exp = simplify(&Expr::Add(
            Box::new(exp.clone()),
            Box::new(Expr::Rational(Rational::one())),
        ));
        return Some(Expr::Div(
            Box::new(Expr::Pow(Box::new(base.clone()), Box::new(new_exp.clone()))),
            Box::new(Expr::Mul(Box::new(a), Box::new(new_exp))),
        ));
    }

    None
}

fn integrate_call(name: &str, arg: &Expr, var: &str) -> Option<Expr> {
    let (a, _b) = extract_linear(arg, var)?;
    let prim = match name {
        "sin" => Expr::UnaryNeg(Box::new(Expr::Call("cos".to_string(), vec![arg.clone()]))),
        "cos" => Expr::Call("sin".to_string(), vec![arg.clone()]),
        "exp" => Expr::Call("exp".to_string(), vec![arg.clone()]),
        "sqrt" => {
            let three_halves = Expr::Rational(Rational::new(3, 2).unwrap());
            return Some(Expr::Div(
                Box::new(Expr::Mul(
                    Box::new(Expr::Rational(Rational::from_int(2))),
                    Box::new(Expr::Pow(Box::new(arg.clone()), Box::new(three_halves))),
                )),
                Box::new(Expr::Mul(
                    Box::new(Expr::Rational(Rational::from_int(3))),
                    Box::new(a),
                )),
            ));
        }
        "ln" | "log" => {
            // int(ln(a*x+b)) = ((a*x+b)*ln(a*x+b) - (a*x+b)) / a
            let ln_arg = Expr::Call("ln".to_string(), vec![arg.clone()]);
            Expr::Sub(
                Box::new(Expr::Mul(Box::new(arg.clone()), Box::new(ln_arg))),
                Box::new(arg.clone()),
            )
        }
        _ => return None,
    };
    Some(Expr::Div(Box::new(prim), Box::new(a)))
}

fn extract_linear(expr: &Expr, var: &str) -> Option<(Expr, Expr)> {
    let s = expand(expr);
    let deriv = differentiate(&s, var).ok()?;
    if depends_on(&deriv, var) || matches!(deriv, Expr::Rational(r) if r.is_zero()) {
        return None;
    }
    let intercept = simplify(&subs(var, &Expr::Rational(Rational::zero()), &s));
    Some((deriv, intercept))
}

fn try_u_substitution(expr: &Expr, var: &str) -> Option<Expr> {
    // Check `num / den` -> `c * ln(den)` when `num / den' = c` (constant).
    if let Expr::Div(num, den) = expr
        && depends_on(den, var)
        && let Ok(dden) = differentiate(den, var)
        && !matches!(dden, Expr::Rational(r) if r.is_zero())
    {
        let ratio = simplify(&Expr::Div(num.clone(), Box::new(dden)));
        if !depends_on(&ratio, var) {
            return Some(Expr::Mul(
                Box::new(ratio),
                Box::new(Expr::Call("ln".to_string(), vec![(**den).clone()])),
            ));
        }
    }

    // Check product `A * B` where `A` or `B` has an inner `u` whose derivative divides the other factor.
    let mut factors = Vec::new();
    collect_mul_factors(expr, &mut factors);
    if factors.len() < 2 {
        return None;
    }
    for idx in 0..factors.len() {
        let cand = &factors[idx];
        let inner = match cand {
            Expr::Call(_, args) if args.len() == 1 && depends_on(&args[0], var) => args[0].clone(),
            Expr::Pow(base, exp) if !depends_on(exp, var) && depends_on(base, var) => {
                (**base).clone()
            }
            Expr::Pow(base, exp) if !depends_on(base, var) && depends_on(exp, var) => {
                (**exp).clone()
            }
            _ => continue,
        };
        let Ok(du) = differentiate(&inner, var) else {
            continue;
        };
        if matches!(du, Expr::Rational(r) if r.is_zero()) {
            continue;
        }
        let mut rest_factors = Vec::new();
        for (j, f) in factors.iter().enumerate() {
            if j != idx {
                rest_factors.push(f.clone());
            }
        }
        let rest = combine_mul_list(rest_factors);
        let ratio = simplify(&Expr::Div(Box::new(rest), Box::new(du)));
        if !depends_on(&ratio, var) {
            let u_sym = "__u_sub__";
            let substituted = subs_expr_once(cand, &inner, &Expr::Symbol(u_sym.to_string()));
            if let Some(u_int) = try_integrate(&substituted, u_sym) {
                let back = subs(u_sym, &inner, &u_int);
                return Some(Expr::Mul(Box::new(ratio), Box::new(back)));
            }
        }
    }
    None
}

fn subs_expr_once(expr: &Expr, target: &Expr, replacement: &Expr) -> Expr {
    if expr == target {
        return replacement.clone();
    }
    match expr {
        Expr::Call(name, args) => Expr::Call(
            name.clone(),
            args.iter()
                .map(|a| subs_expr_once(a, target, replacement))
                .collect(),
        ),
        Expr::Pow(b, e) => Expr::Pow(
            Box::new(subs_expr_once(b, target, replacement)),
            Box::new(subs_expr_once(e, target, replacement)),
        ),
        _ => expr.clone(),
    }
}

fn collect_mul_factors(expr: &Expr, out: &mut Vec<Expr>) {
    match expr {
        Expr::Mul(a, b) => {
            collect_mul_factors(a, out);
            collect_mul_factors(b, out);
        }
        other => out.push(other.clone()),
    }
}

fn try_tabular_ibp(expr: &Expr, var: &str) -> Option<Expr> {
    let mut factors = Vec::new();
    collect_mul_factors(expr, &mut factors);
    if factors.len() < 2 {
        return None;
    }

    // Find one transcendental factor `exp/sin/cos(a*x + b)` and verify remaining product is polynomial in `var`.
    for idx in 0..factors.len() {
        let trans = &factors[idx];
        let is_trans = match trans {
            Expr::Call(n, a) if a.len() == 1 && matches!(n.as_str(), "exp" | "sin" | "cos") => {
                extract_linear(&a[0], var).is_some()
            }
            Expr::Pow(b, e) if matches!(b.as_ref(), Expr::Symbol(s) if s == "e") => {
                extract_linear(e, var).is_some()
            }
            _ => false,
        };
        if !is_trans {
            continue;
        }
        let mut poly_factors = Vec::new();
        for (j, f) in factors.iter().enumerate() {
            if j != idx {
                poly_factors.push(f.clone());
            }
        }
        let poly_expr = expand(&combine_mul_list(poly_factors));
        let poly_coeffs = extract_poly(&poly_expr, var)?;
        if poly_coeffs.len() > 12 {
            return None;
        }
        let mut u = poly_expr;
        let mut v = trans.clone();
        let mut acc = Expr::Rational(Rational::zero());
        let mut sign = 1i128;
        for _ in 0..poly_coeffs.len() {
            v = try_integrate(&v, var)?;
            let term = Expr::Mul(Box::new(u.clone()), Box::new(v.clone()));
            acc = if sign > 0 {
                Expr::Add(Box::new(acc), Box::new(term))
            } else {
                Expr::Sub(Box::new(acc), Box::new(term))
            };
            u = differentiate(&u, var).ok()?;
            if matches!(u, Expr::Rational(r) if r.is_zero()) {
                break;
            }
            sign = -sign;
        }
        return Some(acc);
    }
    None
}

fn split_fraction(expr: &Expr) -> (Expr, Expr) {
    match expr {
        Expr::Div(num, den) => ((**num).clone(), (**den).clone()),
        _ => (expr.clone(), Expr::Rational(Rational::one())),
    }
}

fn is_zero_expr(expr: &Expr) -> bool {
    if matches!(expr, Expr::Rational(r) if r.is_zero()) {
        return true;
    }
    if matches!(expr, Expr::Float(v) if v.abs() < 1e-12) {
        return true;
    }
    if let Ok(v) = evalf(expr) {
        return v.abs() < 1e-12;
    }
    false
}

/// Evaluate symbolic `limit(expr, var = point)` using algebraic simplification and L'Hôpital's rule.
pub fn limit(expr: &Expr, var: &str, point: &Expr) -> Result<Expr> {
    if matches!(point, Expr::Symbol(s) if s == "infinity")
        || matches!(point, Expr::UnaryNeg(inner) if matches!(inner.as_ref(), Expr::Symbol(s) if s == "infinity"))
    {
        return limit_at_infinity(expr, var, point);
    }

    let s = simplify(expr);
    let (mut num, mut den) = split_fraction(&s);

    for _step in 0..8 {
        let sub_num = simplify(&subs(var, point, &num));
        let sub_den = simplify(&subs(var, point, &den));
        if !is_zero_expr(&sub_den) {
            return Ok(simplify(&Expr::Div(Box::new(sub_num), Box::new(sub_den))));
        }
        if !is_zero_expr(&sub_num) {
            bail!("limit `{expr}` as `{var} -> {point}` diverges (non-zero over zero)");
        }
        // 0 / 0 indeterminate form -> apply L'Hôpital's rule.
        num = differentiate(&num, var)?;
        den = differentiate(&den, var)?;
    }

    bail!("could not resolve indeterminate limit `{expr}` as `{var} -> {point}`")
}

fn limit_at_infinity(expr: &Expr, var: &str, point: &Expr) -> Result<Expr> {
    let s = simplify(expr);
    let (num, den) = split_fraction(&s);
    if let (Some(pn), Some(pd)) = (extract_poly(&num, var), extract_poly(&den, var)) {
        let dn = pn.len().saturating_sub(1);
        let dd = pd.len().saturating_sub(1);
        if dn < dd {
            return Ok(Expr::Rational(Rational::zero()));
        }
        if dn == dd {
            return Ok(Expr::Rational(pn[dn] / pd[dd]));
        }
        let neg_inf = matches!(point, Expr::UnaryNeg(_));
        let ratio = (pn[dn] / pd[dd]).to_f64();
        let sign_pos = if neg_inf && !(dn - dd).is_multiple_of(2) {
            ratio < 0.0
        } else {
            ratio > 0.0
        };
        return Ok(if sign_pos {
            Expr::Symbol("infinity".to_string())
        } else {
            Expr::UnaryNeg(Box::new(Expr::Symbol("infinity".to_string())))
        });
    }
    bail!("unsupported limit at infinity for `{expr}`")
}

/// Compute Taylor / Maclaurin series expansion of `expr` around `var = point` up to `order`.
pub fn series(expr: &Expr, var: &str, point: &Expr, order: usize) -> Result<Expr> {
    let mut cur = simplify(expr);
    let mut fact: i128 = 1;
    let mut terms = Vec::new();
    let shifted = if is_zero_expr(point) {
        Expr::Symbol(var.to_string())
    } else {
        simplify(&Expr::Sub(
            Box::new(Expr::Symbol(var.to_string())),
            Box::new(point.clone()),
        ))
    };

    for k in 0..=order {
        if k > 1 {
            fact = fact
                .checked_mul(k as i128)
                .ok_or_else(|| anyhow::anyhow!("factorial overflow in series order {k}"))?;
        }
        let val = simplify(&subs(var, point, &cur));
        if !is_zero_expr(&val) {
            let coeff = simplify(&Expr::Div(
                Box::new(val),
                Box::new(Expr::Rational(Rational::from_int(fact))),
            ));
            let term = match k {
                0 => coeff,
                1 => simplify(&Expr::Mul(Box::new(coeff), Box::new(shifted.clone()))),
                deg => simplify(&Expr::Mul(
                    Box::new(coeff),
                    Box::new(Expr::Pow(
                        Box::new(shifted.clone()),
                        Box::new(Expr::Rational(Rational::from_int(deg as i128))),
                    )),
                )),
            };
            terms.push(term);
        }
        if k < order {
            cur = differentiate(&cur, var)?;
        }
    }

    if terms.is_empty() {
        return Ok(Expr::Rational(Rational::zero()));
    }
    // Preserve ascending power order (`1 + x + 1/2*x^2 + ...`) by folding directly with `Add`.
    let mut iter = terms.into_iter();
    let mut acc = iter.next().unwrap();
    for t in iter {
        acc = Expr::Add(Box::new(acc), Box::new(t));
    }
    Ok(acc)
}

/// Symbolically solve an equation, inequality, or zero-expression for `var`, returning a Maple `List`.
pub fn solve(expr: &Expr, var: &str) -> Result<Expr> {
    if let Expr::Call(name, args) = expr
        && name == "And"
        && args.len() == 2
        && args.iter().all(|a| matches!(a, Expr::Inequality(_, _, _)))
    {
        return solve_and_inequalities(&args[0], &args[1], var);
    }

    if let Expr::Inequality(op, lhs, rhs) = expr {
        return solve_inequality(*op, lhs, rhs, var);
    }

    let target = match expr {
        Expr::Equation(lhs, rhs) => simplify(&Expr::Sub(lhs.clone(), rhs.clone())),
        other => simplify(other),
    };

    if let Some(poly) = extract_poly(&target, var) {
        let roots = solve_polynomial(&poly)?;
        return Ok(Expr::List(roots));
    }

    if let Some(single) = solve_elementary(&target, var) {
        return Ok(Expr::List(vec![simplify(&single)]));
    }

    bail!("unable to symbolically solve `{expr}` for `{var}`")
}

fn solve_polynomial(poly: &[Rational]) -> Result<Vec<Expr>> {
    let deg = poly.len().saturating_sub(1);
    match deg {
        0 => Ok(Vec::new()),
        1 => {
            let root = -(poly[0] / poly[1]);
            Ok(vec![Expr::Rational(root)])
        }
        2 => Ok(solve_quadratic(poly[2], poly[1], poly[0])),
        _ => solve_higher_poly(poly),
    }
}

fn solve_quadratic(a: Rational, b: Rational, c: Rational) -> Vec<Expr> {
    let disc = b * b - Rational::from_int(4) * a * c;
    let two_a = Rational::from_int(2) * a;
    if disc.is_zero() {
        return vec![Expr::Rational(-(b / two_a))];
    }
    if disc.is_positive() {
        let sqrt_d = simplify(&Expr::Call("sqrt".to_string(), vec![Expr::Rational(disc)]));
        let r1 = simplify(&Expr::Div(
            Box::new(Expr::Sub(
                Box::new(Expr::Rational(-b)),
                Box::new(sqrt_d.clone()),
            )),
            Box::new(Expr::Rational(two_a)),
        ));
        let r2 = simplify(&Expr::Div(
            Box::new(Expr::Add(Box::new(Expr::Rational(-b)), Box::new(sqrt_d))),
            Box::new(Expr::Rational(two_a)),
        ));
        let mut pair = vec![r1, r2];
        pair.sort_by(|x, y| {
            evalf(x)
                .unwrap_or(0.0)
                .partial_cmp(&evalf(y).unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pair
    } else {
        let sqrt_neg_d = simplify(&Expr::Call("sqrt".to_string(), vec![Expr::Rational(-disc)]));
        let imag_part = Expr::Mul(
            Box::new(sqrt_neg_d),
            Box::new(Expr::Symbol("I".to_string())),
        );
        let r1 = simplify(&Expr::Div(
            Box::new(Expr::Sub(
                Box::new(Expr::Rational(-b)),
                Box::new(imag_part.clone()),
            )),
            Box::new(Expr::Rational(two_a)),
        ));
        let r2 = simplify(&Expr::Div(
            Box::new(Expr::Add(Box::new(Expr::Rational(-b)), Box::new(imag_part))),
            Box::new(Expr::Rational(two_a)),
        ));
        vec![r1, r2]
    }
}

fn solve_higher_poly(poly: &[Rational]) -> Result<Vec<Expr>> {
    // Factor out rational roots iteratively and finish quadratic remainder if reached.
    let mut rem = poly.to_vec();
    let mut roots = Vec::new();
    while rem.len() > 1 && rem[0].is_zero() {
        roots.push(Expr::Rational(Rational::zero()));
        rem.remove(0);
    }
    while rem.len() > 3 {
        let c0 = rem[0].num.unsigned_abs();
        let cn = rem[rem.len() - 1].num.unsigned_abs();
        let mut found = None;
        for p in small_divisors(c0) {
            for q in small_divisors(cn) {
                if let Ok(r) = Rational::new(p, q) {
                    if eval_poly_rat(&rem, r).is_zero() {
                        found = Some(r);
                        break;
                    }
                    if eval_poly_rat(&rem, -r).is_zero() {
                        found = Some(-r);
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        if let Some(r) = found {
            roots.push(Expr::Rational(r));
            rem = divide_poly_linear(&rem, r);
        } else {
            break;
        }
    }
    if rem.len() == 2 {
        roots.push(Expr::Rational(-(rem[0] / rem[1])));
    } else if rem.len() == 3 {
        roots.extend(solve_quadratic(rem[2], rem[1], rem[0]));
    } else if roots.is_empty() {
        bail!(
            "cannot find closed-form roots for polynomial of degree {}",
            poly.len() - 1
        );
    }
    roots.sort_by(|x, y| {
        evalf(x)
            .unwrap_or(0.0)
            .partial_cmp(&evalf(y).unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(roots)
}

fn small_divisors(n: u128) -> Vec<i128> {
    let mut out = Vec::new();
    let mut d = 1u128;
    while d * d <= n && out.len() < 64 {
        if n.is_multiple_of(d) {
            out.push(d as i128);
            if d * d != n {
                out.push((n / d) as i128);
            }
        }
        d += 1;
    }
    out
}

fn eval_poly_rat(poly: &[Rational], r: Rational) -> Rational {
    let mut acc = Rational::zero();
    for &c in poly.iter().rev() {
        acc = acc * r + c;
    }
    acc
}

fn divide_poly_linear(poly: &[Rational], root: Rational) -> Vec<Rational> {
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

fn solve_elementary(expr: &Expr, var: &str) -> Option<Expr> {
    // Isolates `var` when `expr` splits into `f(var_part) + const = 0`.
    isolate_var(expr, &Expr::Rational(Rational::zero()), var)
}

fn isolate_var(lhs: &Expr, rhs: &Expr, var: &str) -> Option<Expr> {
    if let Expr::Symbol(s) = lhs
        && s == var
    {
        return Some(rhs.clone());
    }
    match lhs {
        Expr::UnaryNeg(a) => isolate_var(a, &Expr::UnaryNeg(Box::new(rhs.clone())), var),
        Expr::Add(a, b) => {
            if !depends_on(b, var) {
                isolate_var(a, &Expr::Sub(Box::new(rhs.clone()), b.clone()), var)
            } else if !depends_on(a, var) {
                isolate_var(b, &Expr::Sub(Box::new(rhs.clone()), a.clone()), var)
            } else {
                None
            }
        }
        Expr::Sub(a, b) => {
            if !depends_on(b, var) {
                isolate_var(a, &Expr::Add(Box::new(rhs.clone()), b.clone()), var)
            } else if !depends_on(a, var) {
                isolate_var(b, &Expr::Sub(a.clone(), Box::new(rhs.clone())), var)
            } else {
                None
            }
        }
        Expr::Mul(a, b) => {
            if !depends_on(b, var) {
                isolate_var(a, &Expr::Div(Box::new(rhs.clone()), b.clone()), var)
            } else if !depends_on(a, var) {
                isolate_var(b, &Expr::Div(Box::new(rhs.clone()), a.clone()), var)
            } else {
                None
            }
        }
        Expr::Div(a, b) if !depends_on(b, var) => {
            isolate_var(a, &Expr::Mul(Box::new(rhs.clone()), b.clone()), var)
        }
        Expr::Call(name, args) if args.len() == 1 => {
            let inv = match name.as_str() {
                "exp" => Expr::Call("ln".to_string(), vec![rhs.clone()]),
                "ln" | "log" => Expr::Call("exp".to_string(), vec![rhs.clone()]),
                "sin" => Expr::Call("arcsin".to_string(), vec![rhs.clone()]),
                "cos" => Expr::Call("arccos".to_string(), vec![rhs.clone()]),
                "tan" => Expr::Call("arctan".to_string(), vec![rhs.clone()]),
                "sqrt" => Expr::Pow(
                    Box::new(rhs.clone()),
                    Box::new(Expr::Rational(Rational::from_int(2))),
                ),
                _ => return None,
            };
            isolate_var(&args[0], &inv, var)
        }
        _ => None,
    }
}

#[derive(Clone, Debug)]
struct IntervalBounds {
    lo: Option<(Expr, bool)>,
    hi: Option<(Expr, bool)>,
}

fn as_abs_arg(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Call(name, args) if name == "abs" && args.len() == 1 => Some(&args[0]),
        _ => None,
    }
}

fn solve_inequality_to_interval(
    op: RelOp,
    lhs: &Expr,
    rhs: &Expr,
    var: &str,
) -> Result<IntervalBounds> {
    let res = solve_inequality(op, lhs, rhs, var)?;
    let Expr::List(items) = res else {
        bail!("unexpected inequality result");
    };
    if items.is_empty() {
        return Ok(IntervalBounds {
            lo: Some((Expr::Rational(Rational::one()), false)),
            hi: Some((Expr::Rational(Rational::zero()), false)),
        });
    }
    if items.len() == 1
        && let Expr::Inequality(rop, rlhs, rrhs) = &items[0]
        && matches!(rlhs.as_ref(), Expr::Symbol(s) if s == var)
    {
        return Ok(match rop {
            RelOp::Lt => IntervalBounds {
                lo: None,
                hi: Some(((**rrhs).clone(), false)),
            },
            RelOp::Le => IntervalBounds {
                lo: None,
                hi: Some(((**rrhs).clone(), true)),
            },
            RelOp::Gt => IntervalBounds {
                lo: Some(((**rrhs).clone(), false)),
                hi: None,
            },
            RelOp::Ge => IntervalBounds {
                lo: Some(((**rrhs).clone(), true)),
                hi: None,
            },
        });
    }
    bail!(
        "inequality `{lhs} {} {rhs}` does not form a single half-line",
        op.ascii()
    )
}

fn solve_and_inequalities(left: &Expr, right: &Expr, var: &str) -> Result<Expr> {
    let Expr::Inequality(op1, l1, r1) = left else {
        bail!("expected inequality, got `{left}`");
    };
    let Expr::Inequality(op2, l2, r2) = right else {
        bail!("expected inequality, got `{right}`");
    };
    let b1 = solve_inequality_to_interval(*op1, l1, r1, var)?;
    let b2 = solve_inequality_to_interval(*op2, l2, r2, var)?;

    let lo = match (b1.lo, b2.lo) {
        (Some(a), Some(b)) => {
            if evalf(&a.0)? >= evalf(&b.0)? {
                Some(a)
            } else {
                Some(b)
            }
        }
        (a, b) => a.or(b),
    };
    let hi = match (b1.hi, b2.hi) {
        (Some(a), Some(b)) => {
            if evalf(&a.0)? <= evalf(&b.0)? {
                Some(a)
            } else {
                Some(b)
            }
        }
        (a, b) => a.or(b),
    };

    match (lo, hi) {
        (Some((lval, linc)), Some((hval, hinc))) => {
            let lv = evalf(&lval)?;
            let hv = evalf(&hval)?;
            if lv > hv || (lv == hv && (!linc || !hinc)) {
                return Ok(Expr::List(Vec::new()));
            }
            if lv == hv {
                return Ok(Expr::List(vec![Expr::Equation(
                    Box::new(Expr::Symbol(var.to_string())),
                    Box::new(lval),
                )]));
            }
            let x = Expr::Symbol(var.to_string());
            let low_cond = Expr::Inequality(
                if linc { RelOp::Le } else { RelOp::Lt },
                Box::new(lval),
                Box::new(x.clone()),
            );
            let high_cond = Expr::Inequality(
                if hinc { RelOp::Le } else { RelOp::Lt },
                Box::new(x),
                Box::new(hval),
            );
            Ok(Expr::List(vec![Expr::Call(
                "And".to_string(),
                vec![low_cond, high_cond],
            )]))
        }
        (Some((lval, linc)), None) => Ok(Expr::List(vec![Expr::Inequality(
            if linc { RelOp::Ge } else { RelOp::Gt },
            Box::new(Expr::Symbol(var.to_string())),
            Box::new(lval),
        )])),
        (None, Some((hval, hinc))) => Ok(Expr::List(vec![Expr::Inequality(
            if hinc { RelOp::Le } else { RelOp::Lt },
            Box::new(Expr::Symbol(var.to_string())),
            Box::new(hval),
        )])),
        (None, None) => Ok(Expr::List(Vec::new())),
    }
}

fn solve_inequality(op: RelOp, lhs: &Expr, rhs: &Expr, var: &str) -> Result<Expr> {
    if let (Some(a), Some(b)) = (as_abs_arg(lhs), as_abs_arg(rhs)) {
        let diff_sq = simplify(&Expr::Sub(
            Box::new(Expr::Pow(
                Box::new(a.clone()),
                Box::new(Expr::Rational(Rational::from_int(2))),
            )),
            Box::new(Expr::Pow(
                Box::new(b.clone()),
                Box::new(Expr::Rational(Rational::from_int(2))),
            )),
        ));
        return solve_inequality(op, &diff_sq, &Expr::Rational(Rational::zero()), var);
    }

    if let Some(inner) = as_abs_arg(lhs) {
        let neg_rhs = simplify(&Expr::UnaryNeg(Box::new(rhs.clone())));
        return match op {
            RelOp::Lt | RelOp::Le => {
                let c1 = Expr::Inequality(op, Box::new(neg_rhs), Box::new(inner.clone()));
                let c2 = Expr::Inequality(op, Box::new(inner.clone()), Box::new(rhs.clone()));
                solve_and_inequalities(&c1, &c2, var)
            }
            RelOp::Gt | RelOp::Ge => {
                let left_op = if op == RelOp::Ge {
                    RelOp::Le
                } else {
                    RelOp::Lt
                };
                let right_op = op;
                let Expr::List(mut left_sol) = solve_inequality(left_op, inner, &neg_rhs, var)?
                else {
                    unreachable!()
                };
                let Expr::List(mut right_sol) = solve_inequality(right_op, inner, rhs, var)? else {
                    unreachable!()
                };
                if left_sol.len() == 1 && right_sol.len() == 1 {
                    let a = left_sol.pop().unwrap();
                    let b = right_sol.pop().unwrap();
                    return Ok(Expr::List(vec![Expr::Call("Or".to_string(), vec![a, b])]));
                }
                left_sol.extend(right_sol);
                Ok(Expr::List(left_sol))
            }
        };
    }

    if as_abs_arg(rhs).is_some() {
        return solve_inequality(op.flip(), rhs, lhs, var);
    }

    let target = simplify(&expand(&Expr::Sub(
        Box::new(lhs.clone()),
        Box::new(rhs.clone()),
    )));
    let x_sym = Expr::Symbol(var.to_string());
    if let Some(poly) = extract_poly(&target, var) {
        let deg = poly.len().saturating_sub(1);
        match deg {
            0 => {
                let c = poly[0];
                let holds = match op {
                    RelOp::Lt => c < Rational::zero(),
                    RelOp::Le => c <= Rational::zero(),
                    RelOp::Gt => c > Rational::zero(),
                    RelOp::Ge => c >= Rational::zero(),
                };
                if holds {
                    return Ok(Expr::List(vec![x_sym]));
                }
                return Ok(Expr::List(Vec::new()));
            }
            1 => {
                let b = poly[0];
                let a = poly[1];
                let root = -(b / a);
                let eff = if a.is_negative() { op.flip() } else { op };
                return Ok(Expr::List(vec![Expr::Inequality(
                    eff,
                    Box::new(x_sym),
                    Box::new(Expr::Rational(root)),
                )]));
            }
            2 => {
                let (c, b, a) = (poly[0], poly[1], poly[2]);
                let roots = solve_quadratic(a, b, c);
                if roots.len() == 2 && evalf(&roots[0]).is_ok() && evalf(&roots[1]).is_ok() {
                    let (mut r1, mut r2) = (roots[0].clone(), roots[1].clone());
                    if evalf(&r1)? > evalf(&r2)? {
                        std::mem::swap(&mut r1, &mut r2);
                    }
                    let inclusive = matches!(op, RelOp::Le | RelOp::Ge);
                    let between = (a.is_positive() && matches!(op, RelOp::Lt | RelOp::Le))
                        || (a.is_negative() && matches!(op, RelOp::Gt | RelOp::Ge));
                    if between {
                        let cop = if inclusive { RelOp::Le } else { RelOp::Lt };
                        return Ok(Expr::List(vec![Expr::Call(
                            "And".to_string(),
                            vec![
                                Expr::Inequality(cop, Box::new(r1), Box::new(x_sym.clone())),
                                Expr::Inequality(cop, Box::new(x_sym), Box::new(r2)),
                            ],
                        )]));
                    } else {
                        let lop = if inclusive { RelOp::Le } else { RelOp::Lt };
                        let rop = if inclusive { RelOp::Ge } else { RelOp::Gt };
                        return Ok(Expr::List(vec![Expr::Call(
                            "Or".to_string(),
                            vec![
                                Expr::Inequality(lop, Box::new(x_sym.clone()), Box::new(r1)),
                                Expr::Inequality(rop, Box::new(x_sym), Box::new(r2)),
                            ],
                        )]));
                    }
                }
            }
            _ => {}
        }
    }

    bail!(
        "unable to symbolically solve inequality `{lhs} {} {rhs}` for `{var}`",
        op.ascii()
    )
}

fn replace_ode_terms(expr: &Expr, y_name: &str, indep: &mut Option<String>) -> Expr {
    match expr {
        Expr::Call(name, args) if name == "diff" && args.len() == 2 => {
            let is_y = matches!(&args[0], Expr::Symbol(s) if s == y_name)
                || matches!(&args[0], Expr::Call(fn_name, _) if fn_name == y_name);
            if is_y {
                match &args[1] {
                    Expr::Symbol(v) => {
                        *indep = Some(v.clone());
                        return Expr::Symbol("__dy1".to_string());
                    }
                    Expr::Call(op, sargs) if op == "$" && sargs.len() == 2 => {
                        if let Expr::Symbol(v) = &sargs[0] {
                            *indep = Some(v.clone());
                        }
                        if matches!(&sargs[1], Expr::Rational(r) if *r == Rational::from_int(2)) {
                            return Expr::Symbol("__dy2".to_string());
                        }
                    }
                    _ => {}
                }
            }
            expr.clone()
        }
        Expr::Call(name, args) if name == y_name && args.len() == 1 => {
            if let Expr::Symbol(v) = &args[0] {
                *indep = Some(v.clone());
            }
            Expr::Symbol("__dy0".to_string())
        }
        Expr::Symbol(s) if s == y_name => Expr::Symbol("__dy0".to_string()),
        Expr::UnaryNeg(a) => Expr::UnaryNeg(Box::new(replace_ode_terms(a, y_name, indep))),
        Expr::Add(a, b) => Expr::Add(
            Box::new(replace_ode_terms(a, y_name, indep)),
            Box::new(replace_ode_terms(b, y_name, indep)),
        ),
        Expr::Sub(a, b) => Expr::Sub(
            Box::new(replace_ode_terms(a, y_name, indep)),
            Box::new(replace_ode_terms(b, y_name, indep)),
        ),
        Expr::Mul(a, b) => Expr::Mul(
            Box::new(replace_ode_terms(a, y_name, indep)),
            Box::new(replace_ode_terms(b, y_name, indep)),
        ),
        Expr::Div(a, b) => Expr::Div(
            Box::new(replace_ode_terms(a, y_name, indep)),
            Box::new(replace_ode_terms(b, y_name, indep)),
        ),
        Expr::Pow(a, b) => Expr::Pow(
            Box::new(replace_ode_terms(a, y_name, indep)),
            Box::new(replace_ode_terms(b, y_name, indep)),
        ),
        other => other.clone(),
    }
}

fn eval_ode_residual(res: &Expr, d2: i128, d1: i128, d0: i128) -> Expr {
    let e = subs("__dy2", &Expr::Rational(Rational::from_int(d2)), res);
    let e = subs("__dy1", &Expr::Rational(Rational::from_int(d1)), &e);
    subs("__dy0", &Expr::Rational(Rational::from_int(d0)), &e)
}

/// Solve ordinary differential equations (`dsolve(ode, y)` or `dsolve([ode, ic], y)`).
pub fn dsolve(ode_spec: &Expr, y_target: &Expr) -> Result<Expr> {
    let y_name = match y_target {
        Expr::Symbol(s) => s.clone(),
        Expr::Call(s, _) => s.clone(),
        other => bail!("`dsolve` target must be function/variable `y`, got `{other}`"),
    };

    let (ode_expr, ics) = match ode_spec {
        Expr::List(items) if !items.is_empty() => (items[0].clone(), &items[1..]),
        other => (other.clone(), &[][..]),
    };

    let raw_residual = match &ode_expr {
        Expr::Equation(lhs, rhs) => Expr::Sub(lhs.clone(), rhs.clone()),
        other => other.clone(),
    };
    let mut indep = None;
    let rep = replace_ode_terms(&raw_residual, &y_name, &mut indep);
    let x_var = indep.unwrap_or_else(|| "x".to_string());
    let x_sym = Expr::Symbol(x_var.clone());

    let f0 = eval_ode_residual(&rep, 0, 0, 0);
    let c_coeff = simplify(&Expr::Sub(
        Box::new(eval_ode_residual(&rep, 0, 0, 1)),
        Box::new(f0.clone()),
    ));
    let b_coeff = simplify(&Expr::Sub(
        Box::new(eval_ode_residual(&rep, 0, 1, 0)),
        Box::new(f0.clone()),
    ));
    let a_coeff = simplify(&Expr::Sub(
        Box::new(eval_ode_residual(&rep, 1, 0, 0)),
        Box::new(f0.clone()),
    ));

    let is_zero = |e: &Expr| matches!(simplify(e), Expr::Rational(r) if r.is_zero());

    if is_zero(&a_coeff) && !is_zero(&b_coeff) && is_zero(&c_coeff) {
        let rhs_fn = simplify(&Expr::Div(
            Box::new(simplify(&Expr::UnaryNeg(Box::new(f0)))),
            Box::new(b_coeff),
        ));
        let anti = integrate(&rhs_fn, &x_var)?;
        let mut sol = simplify(&Expr::Add(
            Box::new(anti),
            Box::new(Expr::Symbol("_C1".to_string())),
        ));
        if let Some(Expr::Equation(ic_l, ic_r)) = ics.first()
            && let Expr::Call(fn_name, ic_args) = ic_l.as_ref()
            && fn_name == &y_name
            && ic_args.len() == 1
        {
            let at_x0 = subs(&x_var, &ic_args[0], &sol);
            let c_val = simplify(&Expr::Sub(
                ic_r.clone(),
                Box::new(subs("_C1", &Expr::Rational(Rational::zero()), &at_x0)),
            ));
            sol = subs("_C1", &c_val, &sol);
        }
        return Ok(Expr::Equation(
            Box::new(y_target.clone()),
            Box::new(simplify(&sol)),
        ));
    }

    if is_zero(&a_coeff) && !is_zero(&b_coeff) && is_zero(&f0) {
        let rate = simplify(&Expr::Div(
            Box::new(Expr::UnaryNeg(Box::new(c_coeff))),
            Box::new(b_coeff),
        ));
        let exp_term = simplify(&Expr::Call(
            "exp".to_string(),
            vec![simplify(&Expr::Mul(Box::new(rate), Box::new(x_sym)))],
        ));
        let mut sol = Expr::Mul(
            Box::new(Expr::Symbol("_C1".to_string())),
            Box::new(exp_term),
        );
        if let Some(Expr::Equation(ic_l, ic_r)) = ics.first()
            && let Expr::Call(fn_name, ic_args) = ic_l.as_ref()
            && fn_name == &y_name
            && ic_args.len() == 1
        {
            let basis0 = subs(
                &x_var,
                &ic_args[0],
                &subs("_C1", &Expr::Rational(Rational::one()), &sol),
            );
            let c_val = simplify(&Expr::Div(ic_r.clone(), Box::new(basis0)));
            sol = subs("_C1", &c_val, &sol);
        }
        return Ok(Expr::Equation(
            Box::new(y_target.clone()),
            Box::new(simplify(&sol)),
        ));
    }

    if !is_zero(&a_coeff) && is_zero(&f0) {
        let disc = simplify(&Expr::Sub(
            Box::new(Expr::Pow(
                Box::new(b_coeff.clone()),
                Box::new(Expr::Rational(Rational::from_int(2))),
            )),
            Box::new(Expr::Mul(
                Box::new(Expr::Rational(Rational::from_int(4))),
                Box::new(Expr::Mul(
                    Box::new(a_coeff.clone()),
                    Box::new(c_coeff.clone()),
                )),
            )),
        ));
        let two_a = simplify(&Expr::Mul(
            Box::new(Expr::Rational(Rational::from_int(2))),
            Box::new(a_coeff.clone()),
        ));
        let d_val = evalf(&disc)?;
        let mut sol = if d_val > 1e-12 {
            let sd = simplify(&Expr::Call("sqrt".to_string(), vec![disc]));
            let r1 = simplify(&Expr::Div(
                Box::new(Expr::Sub(
                    Box::new(Expr::UnaryNeg(Box::new(b_coeff.clone()))),
                    Box::new(sd.clone()),
                )),
                Box::new(two_a.clone()),
            ));
            let r2 = simplify(&Expr::Div(
                Box::new(Expr::Add(
                    Box::new(Expr::UnaryNeg(Box::new(b_coeff))),
                    Box::new(sd),
                )),
                Box::new(two_a),
            ));
            let e1 = Expr::Call(
                "exp".to_string(),
                vec![simplify(&Expr::Mul(Box::new(r1), Box::new(x_sym.clone())))],
            );
            let e2 = Expr::Call(
                "exp".to_string(),
                vec![simplify(&Expr::Mul(Box::new(r2), Box::new(x_sym)))],
            );
            Expr::Add(
                Box::new(Expr::Mul(
                    Box::new(Expr::Symbol("_C1".to_string())),
                    Box::new(e1),
                )),
                Box::new(Expr::Mul(
                    Box::new(Expr::Symbol("_C2".to_string())),
                    Box::new(e2),
                )),
            )
        } else if d_val.abs() <= 1e-12 {
            let r = simplify(&Expr::Div(
                Box::new(Expr::UnaryNeg(Box::new(b_coeff))),
                Box::new(two_a),
            ));
            let e = Expr::Call(
                "exp".to_string(),
                vec![simplify(&Expr::Mul(Box::new(r), Box::new(x_sym.clone())))],
            );
            Expr::Mul(
                Box::new(Expr::Add(
                    Box::new(Expr::Symbol("_C1".to_string())),
                    Box::new(Expr::Mul(
                        Box::new(Expr::Symbol("_C2".to_string())),
                        Box::new(x_sym),
                    )),
                )),
                Box::new(e),
            )
        } else {
            let alpha = simplify(&Expr::Div(
                Box::new(Expr::UnaryNeg(Box::new(b_coeff))),
                Box::new(two_a.clone()),
            ));
            let beta = simplify(&Expr::Div(
                Box::new(simplify(&Expr::Call(
                    "sqrt".to_string(),
                    vec![simplify(&Expr::UnaryNeg(Box::new(disc)))],
                ))),
                Box::new(two_a),
            ));
            let bx = simplify(&Expr::Mul(Box::new(beta), Box::new(x_sym.clone())));
            let osc = Expr::Add(
                Box::new(Expr::Mul(
                    Box::new(Expr::Symbol("_C1".to_string())),
                    Box::new(Expr::Call("cos".to_string(), vec![bx.clone()])),
                )),
                Box::new(Expr::Mul(
                    Box::new(Expr::Symbol("_C2".to_string())),
                    Box::new(Expr::Call("sin".to_string(), vec![bx])),
                )),
            );
            if is_zero(&alpha) {
                osc
            } else {
                let env = Expr::Call(
                    "exp".to_string(),
                    vec![simplify(&Expr::Mul(Box::new(alpha), Box::new(x_sym)))],
                );
                Expr::Mul(Box::new(env), Box::new(osc))
            }
        };

        if ics.len() == 2 {
            let y1 = subs(
                "_C2",
                &Expr::Rational(Rational::zero()),
                &subs("_C1", &Expr::Rational(Rational::one()), &sol),
            );
            let y2 = subs(
                "_C1",
                &Expr::Rational(Rational::zero()),
                &subs("_C2", &Expr::Rational(Rational::one()), &sol),
            );
            let dy1 = differentiate(&y1, &x_var)?;
            let dy2 = differentiate(&y2, &x_var)?;
            if let (Some(Expr::Equation(l1, r1)), Some(Expr::Equation(l2, r2))) =
                (ics.first(), ics.get(1))
                && let (Expr::Call(_, a1), Expr::Call(_, a2)) = (l1.as_ref(), l2.as_ref())
                && a1.len() == 1
                && a2.len() == 1
            {
                let (m11, m12) = (subs(&x_var, &a1[0], &y1), subs(&x_var, &a1[0], &y2));
                let (m21, m22) = (subs(&x_var, &a2[0], &dy1), subs(&x_var, &a2[0], &dy2));
                let det = simplify(&Expr::Sub(
                    Box::new(Expr::Mul(Box::new(m11.clone()), Box::new(m22.clone()))),
                    Box::new(Expr::Mul(Box::new(m12.clone()), Box::new(m21.clone()))),
                ));
                if !is_zero(&det) {
                    let c1 = simplify(&Expr::Div(
                        Box::new(Expr::Sub(
                            Box::new(Expr::Mul(Box::new(r1.as_ref().clone()), Box::new(m22))),
                            Box::new(Expr::Mul(Box::new(m12), Box::new(r2.as_ref().clone()))),
                        )),
                        Box::new(det.clone()),
                    ));
                    let c2 = simplify(&Expr::Div(
                        Box::new(Expr::Sub(
                            Box::new(Expr::Mul(Box::new(m11), Box::new(r2.as_ref().clone()))),
                            Box::new(Expr::Mul(Box::new(r1.as_ref().clone()), Box::new(m21))),
                        )),
                        Box::new(det),
                    ));
                    sol = subs("_C2", &c2, &subs("_C1", &c1, &sol));
                }
            }
        }
        return Ok(Expr::Equation(
            Box::new(y_target.clone()),
            Box::new(simplify(&sol)),
        ));
    }

    bail!("unsupported ODE `{ode_expr}` for `dsolve`")
}

fn poly_mul_rat(a: &[Rational], b: &[Rational]) -> Vec<Rational> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Rational::zero(); a.len() + b.len() - 1];
    for (i, &ca) in a.iter().enumerate() {
        for (j, &cb) in b.iter().enumerate() {
            out[i + j] = out[i + j] + ca * cb;
        }
    }
    out
}

fn poly_div_rem_rat(num: &[Rational], den: &[Rational]) -> (Vec<Rational>, Vec<Rational>) {
    let mut rem = num.to_vec();
    while rem.len() > 1 && rem.last().is_some_and(|c| c.is_zero()) {
        rem.pop();
    }
    let d_deg = den.len() - 1;
    let lc = den[d_deg];
    if rem.len() < den.len() {
        return (vec![Rational::zero()], rem);
    }
    let mut quo = vec![Rational::zero(); rem.len() - d_deg];
    for k in (d_deg..rem.len()).rev() {
        let factor = rem[k] / lc;
        let shift = k - d_deg;
        quo[shift] = factor;
        for j in 0..=d_deg {
            rem[shift + j] = rem[shift + j] - factor * den[j];
        }
    }
    while rem.len() > 1 && rem.last().is_some_and(|c| c.is_zero()) {
        rem.pop();
    }
    (quo, rem)
}

fn poly_to_expr(poly: &[Rational], var: &str) -> Expr {
    let mut acc = Expr::Rational(Rational::zero());
    for (deg, &c) in poly.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        let term = match deg {
            0 => Expr::Rational(c),
            1 => Expr::Mul(
                Box::new(Expr::Rational(c)),
                Box::new(Expr::Symbol(var.to_string())),
            ),
            d => Expr::Mul(
                Box::new(Expr::Rational(c)),
                Box::new(Expr::Pow(
                    Box::new(Expr::Symbol(var.to_string())),
                    Box::new(Expr::Rational(Rational::from_int(d as i128))),
                )),
            ),
        };
        acc = Expr::Add(Box::new(acc), Box::new(term));
    }
    simplify(&acc)
}

/// Partial fraction decomposition of a rational function over rational roots (`convert(f, parfrac, x)`).
pub fn partial_fractions(expr: &Expr, var: &str) -> Result<Expr> {
    let (num_raw, den_raw) = match expr {
        Expr::Div(n, d) => (expand(n), expand(d)),
        other => return Ok(simplify(other)),
    };
    if !depends_on(&den_raw, var) {
        return Ok(simplify(expr));
    }
    let Some(p_poly) = extract_poly(&num_raw, var) else {
        bail!("partial fractions requires polynomial numerator in `{var}`");
    };
    let Some(q_poly) = extract_poly(&den_raw, var) else {
        bail!("partial fractions requires polynomial denominator in `{var}`");
    };
    if q_poly.len() <= 2 {
        return Ok(simplify(expr));
    }

    let q_lc = *q_poly.last().unwrap();
    let p_norm: Vec<Rational> = p_poly.iter().map(|c| *c / q_lc).collect();
    let q_monic: Vec<Rational> = q_poly.iter().map(|c| *c / q_lc).collect();

    let (quo, rem) = poly_div_rem_rat(&p_norm, &q_monic);

    let mut q_work = q_monic.clone();
    let mut root_mults: Vec<(Rational, usize)> = Vec::new();
    while q_work.len() > 1 {
        let mut found_root = None;
        if eval_poly_rat(&q_work, Rational::zero()).is_zero() {
            found_root = Some(Rational::zero());
        } else if let Some(&c0) = q_work.first()
            && let Some(&cn) = q_work.last()
        {
            for p in small_divisors(c0.num.unsigned_abs() * cn.den as u128) {
                for q in small_divisors(cn.num.unsigned_abs() * c0.den as u128) {
                    if let Ok(cand) = Rational::new(p, q) {
                        if eval_poly_rat(&q_work, cand).is_zero() {
                            found_root = Some(cand);
                            break;
                        }
                        if eval_poly_rat(&q_work, -cand).is_zero() {
                            found_root = Some(-cand);
                            break;
                        }
                    }
                }
                if found_root.is_some() {
                    break;
                }
            }
        }
        if let Some(r) = found_root {
            let mut mult = 0usize;
            while eval_poly_rat(&q_work, r).is_zero() && q_work.len() > 1 {
                q_work = divide_poly_linear(&q_work, r);
                mult += 1;
            }
            root_mults.push((r, mult));
        } else {
            break;
        }
    }

    if q_work.len() > 1 && q_work.len() != 3 {
        bail!("denominator does not factor cleanly into linear/quadratic factors over Q");
    }

    let n = q_monic.len() - 1;
    let mut cols: Vec<Vec<Rational>> = Vec::with_capacity(n);
    let mut denom_specs: Vec<(Option<Rational>, usize)> = Vec::with_capacity(n);

    for &(r, mult) in &root_mults {
        for k in 1..=mult {
            let mut factor = vec![Rational::one()];
            for _ in 0..k {
                factor = poly_mul_rat(&factor, &[-r, Rational::one()]);
            }
            let (basis, _) = poly_div_rem_rat(&q_monic, &factor);
            cols.push(basis);
            denom_specs.push((Some(r), k));
        }
    }
    if q_work.len() == 3 {
        let (basis0, _) = poly_div_rem_rat(&q_monic, &q_work);
        let basis1 = poly_mul_rat(&basis0, &[Rational::zero(), Rational::one()]);
        cols.push(basis0);
        denom_specs.push((None, 0));
        cols.push(basis1);
        denom_specs.push((None, 1));
    }

    if cols.len() != n {
        bail!("unable to assemble partial fraction basis");
    }

    let mut aug = vec![vec![Rational::zero(); n + 1]; n];
    for (row, r_val) in rem.iter().enumerate().take(n) {
        aug[row][n] = *r_val;
    }
    for (col_idx, basis) in cols.iter().enumerate() {
        for (row, &val) in basis.iter().enumerate().take(n) {
            aug[row][col_idx] = val;
        }
    }

    for piv in 0..n {
        let Some(swap_row) = (piv..n).find(|&r| !aug[r][piv].is_zero()) else {
            bail!("singular partial fraction linear system");
        };
        aug.swap(piv, swap_row);
        let diag = aug[piv][piv];
        for cell in &mut aug[piv][piv..=n] {
            *cell = *cell / diag;
        }
        let piv_tail = aug[piv][piv..=n].to_vec();
        for (r, row) in aug.iter_mut().enumerate().take(n) {
            if r != piv && !row[piv].is_zero() {
                let factor = row[piv];
                for (cell, &pcell) in row[piv..=n].iter_mut().zip(&piv_tail) {
                    *cell = *cell - factor * pcell;
                }
            }
        }
    }

    let mut total = poly_to_expr(&quo, var);
    let x_sym = Expr::Symbol(var.to_string());
    let mut i = 0;
    while i < denom_specs.len() {
        let coeff = aug[i][n];
        match denom_specs[i] {
            (Some(r), k) => {
                if !coeff.is_zero() {
                    let base = simplify(&Expr::Sub(
                        Box::new(x_sym.clone()),
                        Box::new(Expr::Rational(r)),
                    ));
                    let denom = if k == 1 {
                        base
                    } else {
                        Expr::Pow(
                            Box::new(base),
                            Box::new(Expr::Rational(Rational::from_int(k as i128))),
                        )
                    };
                    let term =
                        simplify(&Expr::Div(Box::new(Expr::Rational(coeff)), Box::new(denom)));
                    total = Expr::Add(Box::new(total), Box::new(term));
                }
                i += 1;
            }
            (None, 0) => {
                let c0 = coeff;
                let c1 = aug[i + 1][n];
                if !c0.is_zero() || !c1.is_zero() {
                    let lin = poly_to_expr(&[c0, c1], var);
                    let quad = poly_to_expr(&q_work, var);
                    total = Expr::Add(
                        Box::new(total),
                        Box::new(Expr::Div(Box::new(lin), Box::new(quad))),
                    );
                }
                i += 2;
            }
            _ => i += 1,
        }
    }

    Ok(simplify(&total))
}

/// Evaluate finite integer or symbolic Faulhaber power summations `sum(f, k = a..b)`.
pub fn summation(expr: &Expr, var: &str, lo: &Expr, hi: &Expr) -> Result<Expr> {
    if let (Expr::Rational(a), Expr::Rational(b)) = (lo, hi)
        && a.is_integer()
        && b.is_integer()
    {
        let start = a.num;
        let end = b.num;
        if end < start {
            return Ok(Expr::Rational(Rational::zero()));
        }
        if end - start <= 10_000 {
            let mut acc = Expr::Rational(Rational::zero());
            for k in start..=end {
                let term = subs(var, &Expr::Rational(Rational::from_int(k)), expr);
                acc = Expr::Add(Box::new(acc), Box::new(term));
            }
            return Ok(simplify(&acc));
        }
    }

    if let Expr::Rational(a) = lo
        && a.is_integer()
        && (a.num == 0 || a.num == 1)
    {
        let expanded = expand(expr);
        if let Some(poly) = extract_poly(&expanded, var)
            && poly.len() <= 5
        {
            let n = hi.clone();
            let n1 = Expr::Add(
                Box::new(n.clone()),
                Box::new(Expr::Rational(Rational::one())),
            );
            let pow_sum = |deg: usize| -> Expr {
                match deg {
                    0 => {
                        if a.num == 0 {
                            n1.clone()
                        } else {
                            n.clone()
                        }
                    }
                    1 => Expr::Div(
                        Box::new(Expr::Mul(Box::new(n.clone()), Box::new(n1.clone()))),
                        Box::new(Expr::Rational(Rational::from_int(2))),
                    ),
                    2 => Expr::Div(
                        Box::new(Expr::Mul(
                            Box::new(n.clone()),
                            Box::new(Expr::Mul(
                                Box::new(n1.clone()),
                                Box::new(Expr::Add(
                                    Box::new(Expr::Mul(
                                        Box::new(Expr::Rational(Rational::from_int(2))),
                                        Box::new(n.clone()),
                                    )),
                                    Box::new(Expr::Rational(Rational::one())),
                                )),
                            )),
                        )),
                        Box::new(Expr::Rational(Rational::from_int(6))),
                    ),
                    3 => Expr::Div(
                        Box::new(Expr::Mul(
                            Box::new(Expr::Pow(
                                Box::new(n.clone()),
                                Box::new(Expr::Rational(Rational::from_int(2))),
                            )),
                            Box::new(Expr::Pow(
                                Box::new(n1.clone()),
                                Box::new(Expr::Rational(Rational::from_int(2))),
                            )),
                        )),
                        Box::new(Expr::Rational(Rational::from_int(4))),
                    ),
                    _ => unreachable!(),
                }
            };

            let mut acc = Expr::Rational(Rational::zero());
            for (deg, &coeff) in poly.iter().enumerate() {
                if !coeff.is_zero() {
                    if deg >= 4 {
                        bail!("symbolic summation supports degree up to 3");
                    }
                    acc = Expr::Add(
                        Box::new(acc),
                        Box::new(Expr::Mul(
                            Box::new(Expr::Rational(coeff)),
                            Box::new(pow_sum(deg)),
                        )),
                    );
                }
            }
            return Ok(expand(&simplify(&acc)));
        }
    }

    bail!("unable to evaluate summation `sum({expr}, {var} = {lo}..{hi})`")
}
