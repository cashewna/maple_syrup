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
