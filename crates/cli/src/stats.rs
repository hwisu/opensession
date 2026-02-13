use anyhow::Result;
use chrono::{Duration, Utc};
use opensession_local_db::{LocalDb, LogFilter};
use std::collections::HashMap;

/// Run the `stats` command.
pub fn run_stats(period: crate::StatsPeriod, format: &crate::output::OutputFormat) -> Result<()> {
    let db = LocalDb::open()?;

    let since = match period {
        crate::StatsPeriod::Day => Some((Utc::now() - Duration::days(1)).to_rfc3339()),
        crate::StatsPeriod::Week => Some((Utc::now() - Duration::weeks(1)).to_rfc3339()),
        crate::StatsPeriod::Month => Some((Utc::now() - Duration::days(30)).to_rfc3339()),
        crate::StatsPeriod::All => None,
    };

    let filter = LogFilter {
        since,
        ..Default::default()
    };
    let sessions = db.list_sessions_log(&filter)?;

    let period_label = match period {
        crate::StatsPeriod::Day => "day",
        crate::StatsPeriod::Week => "week",
        crate::StatsPeriod::Month => "month",
        crate::StatsPeriod::All => "all",
    };

    if sessions.is_empty() {
        eprintln!("No sessions found for period '{period_label}'. Run `opensession index` first.");
        return Ok(());
    }

    // Aggregate stats
    let total_sessions = sessions.len();
    let total_duration: i64 = sessions.iter().map(|s| s.duration_seconds).sum();
    let total_input: i64 = sessions.iter().map(|s| s.total_input_tokens).sum();
    let total_output: i64 = sessions.iter().map(|s| s.total_output_tokens).sum();
    let error_count = sessions.iter().filter(|s| s.has_errors).count();

    // By tool breakdown
    let mut by_tool: HashMap<&str, usize> = HashMap::new();
    for s in &sessions {
        *by_tool.entry(&s.tool).or_insert(0) += 1;
    }

    // Most edited files
    let mut file_counts: HashMap<String, usize> = HashMap::new();
    for s in &sessions {
        if let Some(ref fm) = s.files_modified {
            if let Ok(files) = serde_json::from_str::<Vec<String>>(fm) {
                for f in files {
                    *file_counts.entry(f).or_insert(0) += 1;
                }
            }
        }
    }
    let mut top_files: Vec<(String, usize)> = file_counts.into_iter().collect();
    top_files.sort_by(|a, b| b.1.cmp(&a.1));
    top_files.truncate(5);

    match format {
        crate::output::OutputFormat::Json => print_json(
            total_sessions,
            total_duration,
            total_input,
            total_output,
            error_count,
            &by_tool,
            &top_files,
            period_label,
        )?,
        _ => print_text(
            total_sessions,
            total_duration,
            total_input,
            total_output,
            error_count,
            &by_tool,
            &top_files,
            period_label,
        ),
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn print_text(
    total_sessions: usize,
    total_duration: i64,
    total_input: i64,
    total_output: i64,
    error_count: usize,
    by_tool: &HashMap<&str, usize>,
    top_files: &[(String, usize)],
    period: &str,
) {
    let duration_str = format_duration(total_duration);
    let cost = estimate_cost(total_input, total_output);

    println!("AI Session Stats ({period})");
    println!("{}", "─".repeat(50));
    println!(
        "Sessions: {total_sessions} | Time: {duration_str} | Tokens: {}in / {}out (~${cost:.2})",
        format_k(total_input),
        format_k(total_output),
    );
    println!();

    // By tool bar chart
    if !by_tool.is_empty() {
        println!("By Tool:");
        let max = by_tool.values().copied().max().unwrap_or(1);
        let mut sorted: Vec<(&&str, &usize)> = by_tool.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (tool, count) in &sorted {
            let bar_len = (**count * 20) / max;
            let bar = "█".repeat(bar_len);
            let pct = (**count as f64 / total_sessions as f64) * 100.0;
            println!("  {tool:<15} {bar} {pct:.0}% ({count})");
        }
        println!();
    }

    // Top files
    if !top_files.is_empty() {
        println!("Most edited files:");
        for (file, count) in top_files {
            println!("  {file} ({count} sessions)");
        }
        println!();
    }

    // Error rate
    if error_count > 0 {
        let pct = (error_count as f64 / total_sessions as f64) * 100.0;
        println!("Errors: {error_count}/{total_sessions} sessions ({pct:.0}%)");
    }
}

#[allow(clippy::too_many_arguments)]
fn print_json(
    total_sessions: usize,
    total_duration: i64,
    total_input: i64,
    total_output: i64,
    error_count: usize,
    by_tool: &HashMap<&str, usize>,
    top_files: &[(String, usize)],
    period: &str,
) -> Result<()> {
    let json = serde_json::json!({
        "period": period,
        "total_sessions": total_sessions,
        "total_duration_seconds": total_duration,
        "total_input_tokens": total_input,
        "total_output_tokens": total_output,
        "estimated_cost_usd": estimate_cost(total_input, total_output),
        "error_count": error_count,
        "by_tool": by_tool,
        "top_files": top_files.iter().map(|(f, c)| serde_json::json!({"file": f, "sessions": c})).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        format!("{h}h {m}m")
    }
}

fn format_k(n: i64) -> String {
    if n < 1000 {
        format!("{n}")
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

/// Rough cost estimation based on typical pricing.
fn estimate_cost(input_tokens: i64, output_tokens: i64) -> f64 {
    // Approximate blended pricing (varies by model, this is a rough average)
    let input_cost_per_million = 3.0; // $3/M input tokens (average)
    let output_cost_per_million = 15.0; // $15/M output tokens (average)
    (input_tokens as f64 / 1_000_000.0) * input_cost_per_million
        + (output_tokens as f64 / 1_000_000.0) * output_cost_per_million
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3599), "59m 59s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(7200), "2h 0m");
    }

    #[test]
    fn test_format_k_small() {
        assert_eq!(format_k(0), "0");
        assert_eq!(format_k(500), "500");
        assert_eq!(format_k(999), "999");
    }

    #[test]
    fn test_format_k_thousands() {
        assert_eq!(format_k(1000), "1.0K");
        assert_eq!(format_k(1500), "1.5K");
        assert_eq!(format_k(999_999), "1000.0K");
    }

    #[test]
    fn test_format_k_millions() {
        assert_eq!(format_k(1_000_000), "1.0M");
        assert_eq!(format_k(2_500_000), "2.5M");
    }

    #[test]
    fn test_estimate_cost_zero() {
        assert_eq!(estimate_cost(0, 0), 0.0);
    }

    #[test]
    fn test_estimate_cost_one_million() {
        let cost = estimate_cost(1_000_000, 0);
        assert!((cost - 3.0).abs() < 0.01); // $3/M input
    }

    #[test]
    fn test_estimate_cost_output() {
        let cost = estimate_cost(0, 1_000_000);
        assert!((cost - 15.0).abs() < 0.01); // $15/M output
    }

    #[test]
    fn test_estimate_cost_combined() {
        let cost = estimate_cost(1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.01); // $3 + $15
    }
}
