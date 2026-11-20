use maple_gpui::ui::run_gui;
use maple_gpui::worksheet::{CellOutput, Worksheet};
use std::env;
use std::process::ExitCode;

fn print_help() {
    println!(
        "maple-gpui {} — Cleanroom Maple-inspired Symbolic CAS (GPUI)\n\n\
         USAGE:\n  \
           maple-gpui                 Launch interactive GPUI worksheet window\n  \
           maple-gpui --eval <CODE>   Evaluate Maple statements headlessly and exit\n  \
           maple-gpui --help          Show this help message\n  \
           maple-gpui --version       Show version",
        env!("CARGO_PKG_VERSION")
    );
}

fn eval_headless(script: &str) -> ExitCode {
    let mut ws = Worksheet::new();
    let cells = ws.evaluate(script);
    let mut had_error = false;
    for cell in &cells {
        println!("[> {}  ({} μs)", cell.input, cell.elapsed_us);
        match &cell.output {
            CellOutput::Text(t) => println!("    {t}"),
            CellOutput::MatrixGrid(rows) => {
                for row in rows {
                    println!("    [ {} ]", row.join("  "));
                }
            }
            CellOutput::Plot(plot) => {
                println!(
                    "    Plot({}) x in [{:.3}, {:.3}], y in [{:.3}, {:.3}], {} series",
                    plot.title,
                    plot.x_min,
                    plot.x_max,
                    plot.y_min,
                    plot.y_max,
                    plot.series.len()
                );
            }
            CellOutput::Suppressed => {}
            CellOutput::Error(err) => {
                eprintln!("    Error: {err}");
                had_error = true;
            }
        }
    }
    if had_error {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    if let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            print_help();
            return ExitCode::SUCCESS;
        }
        if arg == "--version" || arg == "-V" {
            println!("maple-gpui {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        if arg == "--eval" {
            let Some(script) = args.next() else {
                eprintln!("Error: --eval requires a Maple script string");
                return ExitCode::FAILURE;
            };
            return eval_headless(&script);
        }
        if let Some(script) = arg.strip_prefix("--eval=") {
            return eval_headless(script);
        }
        eprintln!("Error: unknown argument `{arg}`. Try --help.");
        return ExitCode::FAILURE;
    }

    run_gui();
    ExitCode::SUCCESS
}
