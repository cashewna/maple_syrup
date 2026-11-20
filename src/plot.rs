use crate::ast::Expr;
use crate::simplify::{evalf, subs};
use anyhow::{Result, bail};

/// Sampled 2D function curve rendered on a GPUI canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotSeries {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

/// Complete 2D plot payload returned by `plot(...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotData {
    pub title: String,
    pub x_label: String,
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub series: Vec<PlotSeries>,
}

/// Sample one or more expressions over `var = a..b` into `PlotData`.
pub fn sample_plot(expr_arg: &Expr, var: &str, a: &Expr, b: &Expr) -> Result<PlotData> {
    let x_min = evalf(a)?;
    let x_max = evalf(b)?;
    if !x_min.is_finite() || !x_max.is_finite() || x_max <= x_min {
        bail!("invalid plot range `{a}..{b}`");
    }

    let targets: Vec<Expr> = match expr_arg {
        Expr::List(items) if !items.is_empty() => items.clone(),
        other => vec![other.clone()],
    };

    const SAMPLES: usize = 240;
    let mut series = Vec::with_capacity(targets.len());
    let mut global_y_min = f64::INFINITY;
    let mut global_y_max = f64::NEG_INFINITY;

    for target in &targets {
        let mut points = Vec::with_capacity(SAMPLES + 1);
        for i in 0..=SAMPLES {
            let t = i as f64 / SAMPLES as f64;
            let x = x_min + t * (x_max - x_min);
            if let Ok(y) = evalf(&subs(var, &Expr::Float(x), target))
                && y.is_finite()
                && y.abs() <= 1e6
            {
                if y < global_y_min {
                    global_y_min = y;
                }
                if y > global_y_max {
                    global_y_max = y;
                }
                points.push((x, y));
            }
        }
        if points.is_empty() {
            bail!("plot expression `{target}` produced no finite points on {x_min}..{x_max}");
        }
        series.push(PlotSeries {
            name: target.to_string(),
            points,
        });
    }

    if (global_y_max - global_y_min).abs() < 1e-9 {
        global_y_min -= 1.0;
        global_y_max += 1.0;
    }

    let title = if series.len() == 1 {
        format!("y = {}", series[0].name)
    } else {
        series
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    Ok(PlotData {
        title,
        x_label: var.to_string(),
        x_min,
        x_max,
        y_min: global_y_min,
        y_max: global_y_max,
        series,
    })
}

/// Complete 3D surface grid payload returned by `plot3d(expr, x = a..b, y = c..d)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Plot3dData {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub z_min: f64,
    pub z_max: f64,
    pub rows: usize,
    pub cols: usize,
    pub grid: Vec<Vec<(f64, f64, f64)>>,
}

/// Sample a two-variable expression over `x = ax..bx, y = ay..by` into a 3D surface mesh.
pub fn sample_plot3d(
    expr: &Expr,
    x_var: &str,
    x_a: &Expr,
    x_b: &Expr,
    y_var: &str,
    y_a: &Expr,
    y_b: &Expr,
) -> Result<Plot3dData> {
    let x_min = evalf(x_a)?;
    let x_max = evalf(x_b)?;
    let y_min = evalf(y_a)?;
    let y_max = evalf(y_b)?;
    if !x_min.is_finite() || !x_max.is_finite() || x_max <= x_min {
        bail!("invalid 3D x-range `{x_a}..{x_b}`");
    }
    if !y_min.is_finite() || !y_max.is_finite() || y_max <= y_min {
        bail!("invalid 3D y-range `{y_a}..{y_b}`");
    }

    const STEPS: usize = 18;
    let mut grid = Vec::with_capacity(STEPS + 1);
    let mut z_min = f64::INFINITY;
    let mut z_max = f64::NEG_INFINITY;
    let mut finite_count = 0usize;

    for i in 0..=STEPS {
        let u = i as f64 / STEPS as f64;
        let x = x_min + u * (x_max - x_min);
        let with_x = subs(x_var, &Expr::Float(x), expr);
        let mut row = Vec::with_capacity(STEPS + 1);
        for j in 0..=STEPS {
            let v = j as f64 / STEPS as f64;
            let y = y_min + v * (y_max - y_min);
            let z = match evalf(&subs(y_var, &Expr::Float(y), &with_x)) {
                Ok(zv) if zv.is_finite() && zv.abs() <= 1e6 => {
                    finite_count += 1;
                    z_min = z_min.min(zv);
                    z_max = z_max.max(zv);
                    zv
                }
                _ => f64::NAN,
            };
            row.push((x, y, z));
        }
        grid.push(row);
    }

    if finite_count == 0 {
        bail!("plot3d expression `{expr}` produced no finite points");
    }
    if (z_max - z_min).abs() < 1e-9 {
        z_min -= 1.0;
        z_max += 1.0;
    }

    Ok(Plot3dData {
        title: format!("z = {expr}"),
        x_label: x_var.to_string(),
        y_label: y_var.to_string(),
        x_min,
        x_max,
        y_min,
        y_max,
        z_min,
        z_max,
        rows: STEPS + 1,
        cols: STEPS + 1,
        grid,
    })
}
