//! Command-line entry point for CU-indexed coverage report generation.

use std::{env, fs, process};

use async_opcua_cu_coverage_report::{generate_markdown_report, parse_snapshot};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 2 && args.len() != 3 {
        return Err(format!(
            "usage: {} <opcua-profile-normalized-snapshot.json> [output.md]",
            args.first()
                .map(String::as_str)
                .unwrap_or("async-opcua-cu-coverage-report")
        ));
    }

    let input = fs::read_to_string(&args[1])
        .map_err(|error| format!("failed to read {}: {error}", args[1]))?;
    let snapshot =
        parse_snapshot(&input).map_err(|error| format!("failed to parse snapshot: {error}"))?;
    let report = generate_markdown_report(&snapshot);

    if let Some(output_path) = args.get(2) {
        fs::write(output_path, report)
            .map_err(|error| format!("failed to write {output_path}: {error}"))?;
    } else {
        print!("{report}");
    }

    Ok(())
}
