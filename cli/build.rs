fn main() {
    println!("cargo:rerun-if-changed=.env");

    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                println!(
                    "cargo:rustc-env={}={}",
                    key.trim(),
                    value.trim().trim_matches('"')
                );
            }
        }
    }
}
