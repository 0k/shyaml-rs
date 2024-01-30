use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::Command;

fn main() {
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .expect("Failed to execute rustc");
    // remove the trailing newline
    let rustc_version = &rustc_version.stdout[..rustc_version.stdout.len() - 1];
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("rustc_version.rs");
    let mut f = File::create(&dest_path).unwrap();

    write!(
        f,
        "pub const RUSTC_VERSION: &str = \"{}\";",
        String::from_utf8_lossy(&rustc_version)
    )
    .unwrap();
}
