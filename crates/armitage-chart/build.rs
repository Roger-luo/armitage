use std::path::Path;

const D3_URL: &str = "https://cdn.jsdelivr.net/npm/d3@7/dist/d3.min.js";
const MARKED_URL: &str = "https://cdn.jsdelivr.net/npm/marked@15/marked.min.js";

fn download(url: &str, dest: &Path) {
    if dest.exists() {
        return;
    }
    let name = dest.file_name().unwrap().to_str().unwrap();
    println!("cargo::warning=Downloading {name}...");
    let mut resp = ureq::get(url)
        .call()
        .unwrap_or_else(|e| panic!("failed to download {name}: {e}"));
    let body = resp
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|e| panic!("failed to read {name}: {e}"));
    std::fs::write(dest, body).unwrap_or_else(|e| panic!("failed to write {name}: {e}"));
}

fn main() {
    let js_dir = Path::new("js");
    download(D3_URL, &js_dir.join("d3.min.js"));
    download(MARKED_URL, &js_dir.join("marked.min.js"));
}
