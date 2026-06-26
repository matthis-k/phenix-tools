use crate::checks::CheckOutcome;
use crate::execute::ExecutionResult;

pub fn print_results(results: &[ExecutionResult], verbose: bool) -> (usize, usize, usize) {
    let mut failed = 0usize;
    let mut passed = 0usize;
    let mut skipped = 0usize;

    for r in results {
        match &r.outcome {
            CheckOutcome::Skipped { .. } => {
                skipped += 1;
            }
            CheckOutcome::Passed => {
                passed += 1;
            }
            CheckOutcome::Failed { reason } | CheckOutcome::Errored { reason } => {
                failed += 1;
                println!("FAILED {}", r.task_id);
                if !r.description.is_empty() {
                    println!("  description: {}", r.description);
                }
                println!("  phase: {}", r.phase);
                println!("  kind: {}", r.kind);
                if !reason.is_empty() {
                    println!("  reason: {}", reason);
                }
                if !r.stdout.is_empty() && verbose {
                    for line in r.stdout.lines() {
                        println!("  stdout: {}", line);
                    }
                }
                if !r.stderr.is_empty() {
                    for line in r.stderr.lines() {
                        println!("  stderr: {}", line);
                    }
                }
                println!();
            }
        }
    }

    if verbose {
        for r in results {
            if r.outcome.is_pass() {
                println!("PASSED {}", r.task_id);
            }
        }
    }

    println!("Summary:");
    println!("  failed: {}", failed);
    println!("  passed: {}", passed);
    println!("  skipped: {}", skipped);

    (failed, passed, skipped)
}
