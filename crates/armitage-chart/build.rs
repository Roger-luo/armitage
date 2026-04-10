use std::path::Path;

const ECHARTS_URL: &str = "https://cdn.jsdelivr.net/npm/echarts@5/dist/echarts.min.js";

fn main() {
    let out = Path::new("js").join("echarts.min.js");
    if out.exists() {
        return;
    }
    println!("cargo::warning=Downloading echarts.min.js...");
    let mut resp = ureq::get(ECHARTS_URL)
        .call()
        .expect("failed to download echarts.min.js");
    let body = resp
        .body_mut()
        .read_to_string()
        .expect("failed to read response");
    std::fs::write(&out, body).expect("failed to write echarts.min.js");
}
