use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=frontend");

    let frontend_dir = Path::new("frontend");
    let dist_dir = frontend_dir.join("dist");

    let is_empty = dist_dir
        .read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true);

    if is_empty && frontend_dir.join("package.json").exists() {
        run_npm(["install"], frontend_dir);
        run_npm(["run", "build"], frontend_dir);
    }
}

fn run_npm<const N: usize>(args: [&str; N], dir: &Path) {
    let status = Command::new("npm")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|error| panic!("npm 실행 실패: {error}"));

    assert!(status.success(), "npm 명령 실패");
}
