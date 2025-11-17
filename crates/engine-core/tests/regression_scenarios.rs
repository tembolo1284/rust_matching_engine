// crates/engine-core/tests/regression_scenarios.rs

use engine_core::MatchingEngine;
use engine_protocol::csv_codec::{format_output_legacy, parse_input_line};

fn load_lines_without_comments(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}

#[test]
fn full_input_matches_reference_output() {
    // Original C++ input and expected output.
    const INPUT: &str = include_str!("data/inputFile.csv");
    const EXPECTED: &str = include_str!("data/output_file.csv");

    let expected_lines = load_lines_without_comments(EXPECTED);

    let mut engine = MatchingEngine::new();
    let mut actual_lines: Vec<String> = Vec::new();

    for raw_line in INPUT.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // parse_input_line: &str -> Option<InputMessage>
        if let Some(input_msg) = parse_input_line(line) {
            let outputs = engine.process_message(input_msg);

            // Use the *legacy* formatter so the output matches the original C++ CSV format.
            for out in outputs {
                let s = format_output_legacy(&out);
                actual_lines.push(s);
            }
        }
    }

    assert_eq!(
        actual_lines, expected_lines,
        "Rust engine output does not match original output_file.csv"
    );
}

