use crate::execute::ExecutionResult;

pub fn print_results(results: &[ExecutionResult], verbose: bool) -> (usize, usize, usize) {
    let mut failed = 0usize;
    let mut passed = 0usize;
    let mut skipped = 0usize;

    for r in results {
        if r.skipped {
            skipped += 1;
            continue;
        }

        if r.passed {
            passed += 1;
        } else {
            failed += 1;
            println!("FAILED {}", r.task_id);
            if !r.description.is_empty() {
                println!("  description: {}", r.description);
            }
            println!("  phase: {}", r.phase);
            println!("  kind: {}", r.kind);
            if !r.reason.is_empty() {
                println!("  reason: {}", r.reason);
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

    if verbose {
        for r in results {
            if r.passed && !r.skipped {
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
