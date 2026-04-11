use std::path::Path;

const D3_URL: &str = "https://cdn.jsdelivr.net/npm/d3@7/dist/d3.min.js";

fn main() {
    let out = Path::new("js").join("d3.min.js");
    if out.exists() {
        return;
    }
    println!("cargo::warning=Downloading d3.min.js...");
    let mut resp = ureq::get(D3_URL)
        .call()
        .expect("failed to download d3.min.js");
    let body = resp
        .body_mut()
        .read_to_string()
        .expect("failed to read response");
    std::fs::write(&out, body).expect("failed to write d3.min.js");
}
