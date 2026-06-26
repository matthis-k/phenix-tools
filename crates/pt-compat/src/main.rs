mod gate;

fn main() {
    eprintln!("warning: `phenix-tools` / `pt` is deprecated. Use `tend` and `stitch` instead.");
    eprintln!("  pt/tend operations  ->  tend plan/run/explain");
    eprintln!("  pt completions      ->  deprecated (use shell built-in)");
    eprintln!();

    let workspace_root = &std::env::current_dir().unwrap_or_default();
    if let Err(e) = gate::dispatch_stub(workspace_root, None) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
