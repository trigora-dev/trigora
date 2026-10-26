use super::{FaultInjection, Report};
use crate::error::CliError;

pub fn render(report: &Report, verify: bool) -> String {
    let mut lines = Vec::new();
    if verify {
        lines.push(format!("Trigora verify — {}", report.program));
        lines.push(String::new());
        lines.push("Baseline".to_string());
        lines.push(row(
            "Durable boundaries",
            &format_int(report.baseline.durable_boundaries),
        ));
        lines.push(row("Status", &report.baseline.status));
        if let Some(faults) = &report.fault_injection {
            lines.push(String::new());
            lines.push("Recovery".to_string());
            lines.push(row(
                "Fault points tested",
                &format!(
                    "{} / {}",
                    format_int(faults.fault_points_tested),
                    format_int(faults.fault_points_total)
                ),
            ));
            lines.push(row(
                "Successful recoveries",
                &format_int(faults.successful_recoveries),
            ));
            lines.push(row(
                "Failed recoveries",
                &format_int(faults.failed_recoveries),
            ));
            lines.push(row(
                "Result mismatches",
                &format_int(faults.result_mismatches),
            ));
            lines.push(row(
                "Duplicate effects",
                &format_int(faults.duplicate_effects),
            ));
            lines.push(row("Missing effects", &format_int(faults.missing_effects)));
            lines.push(row(
                "Effect calls",
                &format_int(faults.provider_invocations),
            ));
            lines.push(row("Reused effects", &format_int(faults.journal_hits)));
            lines.push(row("Median recovery", &format_ms(faults.recovery_ms.p50)));
            lines.push(row("p95 recovery", &format_ms(faults.recovery_ms.p95)));
            lines.push(row("p99 recovery", &format_ms(faults.recovery_ms.p99)));
            lines.push(String::new());
            lines.push(verify_verdict(faults));
        }
    } else {
        lines.push(format!("Trigora benchmark — {}", report.program));
        lines.push(String::new());
        lines.push("Execution".to_string());
        lines.push(row(
            "Durable boundaries",
            &format_int(report.baseline.durable_boundaries),
        ));
        lines.push(row(
            "Healthy runtime",
            &format_ms(Some(report.baseline.healthy_runtime_ms)),
        ));
        let overhead = report.baseline.checkpoint_ms.total.unwrap_or(0.0);
        let share = if report.baseline.healthy_runtime_ms > 0.0 {
            format!(
                "  ({:.1}%)",
                100.0 * overhead / report.baseline.healthy_runtime_ms
            )
        } else {
            String::new()
        };
        lines.push(row(
            "Checkpoint overhead",
            &format!("{}{share}", format_ms(Some(overhead))),
        ));
        lines.push(String::new());
        lines.push("Continuation".to_string());
        lines.push(row(
            "Median size",
            &format_bytes(report.baseline.continuation_bytes.p50),
        ));
        lines.push(row(
            "p95 size",
            &format_bytes(report.baseline.continuation_bytes.p95),
        ));
        lines.push(row(
            "Maximum size",
            &format_bytes(report.baseline.continuation_bytes.max),
        ));
        lines.push(String::new());
        lines.push("Continuation restore".to_string());
        lines.push(row(
            "p50",
            &format_ms(report.baseline.continuation_restore_ms.p50),
        ));
        lines.push(row(
            "p95",
            &format_ms(report.baseline.continuation_restore_ms.p95),
        ));
        lines.push(row(
            "p99",
            &format_ms(report.baseline.continuation_restore_ms.p99),
        ));
        lines.push(String::new());
        lines.push("Effects".to_string());
        lines.push(row(
            "Effect calls",
            &format_int(report.baseline.provider_invocations),
        ));
        lines.push(row(
            "Reused effects",
            &format_int(report.baseline.journal_hits),
        ));
        lines.push(row(
            "Events awaited",
            &format_int(report.baseline.events_awaited),
        ));
        lines.push(row("CPU time", &format_ms(Some(report.baseline.cpu_ms))));
    }
    lines.push(String::new());
    lines.push(
        "Sleeps are fast-forwarded. Healthy runtime does not include sleep delays.".to_string(),
    );
    lines.push(String::new());
    lines.join("\n")
}

pub fn verdict_error(report: &Report) -> Option<CliError> {
    let Some(faults) = &report.fault_injection else {
        return None;
    };
    if faults.failed_recoveries == 0
        && faults.result_mismatches == 0
        && faults.duplicate_effects == 0
        && faults.missing_effects == 0
    {
        return None;
    }
    Some(
        CliError::new("Recovery check failed")
            .detail("Failed recoveries", faults.failed_recoveries.to_string())
            .detail("Result mismatches", faults.result_mismatches.to_string())
            .detail("Duplicate effects", faults.duplicate_effects.to_string())
            .detail("Missing effects", faults.missing_effects.to_string()),
    )
}

fn verify_verdict(faults: &FaultInjection) -> String {
    if faults.failed_recoveries == 0
        && faults.result_mismatches == 0
        && faults.duplicate_effects == 0
        && faults.missing_effects == 0
    {
        if faults.mode == "all" {
            "✔ Recovery verified at every checkpoint".to_string()
        } else {
            format!(
                "✔ Recovery verified at {} sampled checkpoints",
                format_int(faults.fault_points_tested)
            )
        }
    } else {
        "✖ Recovery check failed".to_string()
    }
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<26}{value}")
}

fn format_int(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::new();
    for (index, ch) in raw.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_ms(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    if value < 1.0 {
        format!("{value:.3} ms")
    } else if value < 100.0 {
        format!("{value:.1} ms")
    } else {
        format!("{value:.0} ms")
    }
}

fn format_bytes(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    if value >= 1024.0 {
        format!("{:.1} KB", value / 1024.0)
    } else {
        format!("{value:.0} B")
    }
}
