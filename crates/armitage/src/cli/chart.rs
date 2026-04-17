use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, mpsc};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};
use serde_json;

use crate::error::Result;
use armitage_chart::data::IssueDates;
use armitage_core::org::Org;
use armitage_core::tree::{find_org_root, walk_nodes};

/// Script injected into the chart HTML for live reload in watch mode.
const LIVE_RELOAD_SCRIPT: &str = r"
<script>
(function() {
  let lastVersion = 0;
  setInterval(function() {
    fetch('/__version')
      .then(r => r.text())
      .then(v => {
        const ver = parseInt(v, 10);
        if (lastVersion > 0 && ver > lastVersion) location.reload();
        lastVersion = ver;
      })
      .catch(() => {});
  }, 500);
})();
</script>
";

pub fn run_chart(
    output: Option<String>,
    no_open: bool,
    offline: bool,
    no_serve: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    // --output implies --no-serve (write to file)
    let write_to_file = no_serve || output.is_some();

    if write_to_file {
        let output_path = output.map_or_else(
            || org_root.join(".armitage").join("chart.html"),
            PathBuf::from,
        );

        let html = generate_chart(&org_root, offline)?;
        write_file(&output_path, &html)?;
        eprintln!("Chart written to {}", output_path.display());

        if !no_open {
            open_url(&format!("file://{}", output_path.display()));
        }
    } else {
        run_watch_server(&org_root, offline)?;
    }

    Ok(())
}

fn generate_chart(org_root: &Path, offline: bool) -> Result<String> {
    let org = Org::open(org_root)?;
    let entries = walk_nodes(org_root)?;
    let issue_dates = build_issue_dates_map(org_root);
    let chart_data = armitage_chart::build_chart_data(&entries, &org.info().name, &issue_dates)?;
    armitage_chart::render_chart(&chart_data, offline).map_err(Into::into)
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

fn run_watch_server(org_root: &Path, offline: bool) -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| crate::error::Error::Other(format!("bind error: {e}")))?;
    let addr = listener
        .local_addr()
        .map_err(|e| crate::error::Error::Other(format!("addr error: {e}")))?;
    let url = format!("http://{addr}");

    // Shared state: chart HTML + version counter
    let html_store: Arc<RwLock<String>> = Arc::new(RwLock::new(String::new()));
    let version: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));

    // Generate initial chart
    let initial = generate_chart(org_root, offline)?;
    *html_store.write().unwrap() = inject_live_reload(&initial);

    eprintln!("Serving chart at {url}");
    eprintln!("Watching for changes... (press Ctrl+C to stop)");
    open_url(&url);

    // Spawn HTTP server thread
    let html_for_server = Arc::clone(&html_store);
    let version_for_server = Arc::clone(&version);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            serve_request(stream, &html_for_server, &version_for_server);
        }
    });

    // File watcher — only watch directories containing relevant files,
    // not the entire org tree (which may include .claude/, .git/, etc.)
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .map_err(|e| crate::error::Error::Other(format!("watch error: {e}")))?;

    // Watch org root (non-recursive) for armitage.toml, labels.toml, team.toml
    watcher
        .watch(org_root, RecursiveMode::NonRecursive)
        .map_err(|e| crate::error::Error::Other(format!("watch error: {e}")))?;

    // Watch each node directory (contains node.toml, milestones.toml, etc.)
    for entry in walk_nodes(org_root)? {
        let _ = watcher.watch(&entry.dir, RecursiveMode::NonRecursive);
    }

    // Watch triage DB directory
    let triage_dir = org_root.join(".armitage").join("triage");
    if triage_dir.exists() {
        let _ = watcher.watch(&triage_dir, RecursiveMode::NonRecursive);
    }

    let mut last_rebuild = Instant::now();
    let debounce = Duration::from_millis(500);

    while let Ok(event) = rx.recv() {
        if !is_relevant_change(&event) {
            continue;
        }
        if last_rebuild.elapsed() < debounce {
            while rx.try_recv().is_ok() {}
            continue;
        }
        std::thread::sleep(Duration::from_millis(200));
        while rx.try_recv().is_ok() {}

        match generate_chart(org_root, offline) {
            Ok(html) => {
                *html_store.write().unwrap() = inject_live_reload(&html);
                version.fetch_add(1, Ordering::Relaxed);
                last_rebuild = Instant::now();
                eprintln!("Chart rebuilt (v{})", version.load(Ordering::Relaxed));
            }
            Err(e) => {
                eprintln!("  rebuild error: {e}");
            }
        }
    }

    Ok(())
}

fn inject_live_reload(html: &str) -> String {
    // Insert the live-reload script before </body>
    html.rfind("</body>").map_or_else(
        || format!("{html}{LIVE_RELOAD_SCRIPT}"),
        |pos| {
            let mut out = String::with_capacity(html.len() + LIVE_RELOAD_SCRIPT.len());
            out.push_str(&html[..pos]);
            out.push_str(LIVE_RELOAD_SCRIPT);
            out.push_str(&html[pos..]);
            out
        },
    )
}

fn serve_request(
    mut stream: std::net::TcpStream,
    html_store: &RwLock<String>,
    version: &AtomicU64,
) {
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);

    let (status, content_type, body) = if request.starts_with("GET /__version") {
        let v = version.load(Ordering::Relaxed).to_string();
        ("200 OK", "text/plain", v)
    } else if request.starts_with("GET / ") || request.starts_with("GET /index") {
        let html = html_store.read().unwrap().clone();
        ("200 OK", "text/html; charset=utf-8", html)
    } else {
        ("404 Not Found", "text/plain", "Not found".to_string())
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn is_relevant_change(event: &notify::Event) -> bool {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }
    event.paths.iter().any(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name == "node.toml"
            || name == "issues.toml"
            || name == "milestones.toml"
            || name == "armitage.toml"
            || name == "labels.toml"
            || name == "triage.db"
            || name == "team.toml"
    })
}

/// Build a map of issue_ref -> IssueDates from the triage DB's project items.
fn build_issue_dates_map(org_root: &Path) -> HashMap<String, IssueDates> {
    let mut map = HashMap::new();
    let Ok(conn) = armitage_triage::db::open_db(org_root) else {
        return map;
    };
    // Query all issues with their state, LEFT JOIN project items for dates
    let Ok(mut stmt) = conn.prepare(
        "SELECT i.repo, i.number, i.state, p.start_date, p.target_date, i.body, i.labels_json, i.author, i.assignees_json, i.is_pr
         FROM issues i
         LEFT JOIN issue_project_items p ON p.issue_id = i.id",
    ) else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let repo: String = row.get(0)?;
        let number: i64 = row.get(1)?;
        let state: String = row.get(2)?;
        let start_date: Option<String> = row.get(3)?;
        let target_date: Option<String> = row.get(4)?;
        let body: String = row.get(5)?;
        let labels_json: String = row.get(6)?;
        let author: String = row.get(7)?;
        let assignees_json: String = row.get(8)?;
        let is_pr: bool = row.get::<_, i64>(9)? != 0;
        Ok((
            format!("{repo}#{number}"),
            state,
            start_date,
            target_date,
            body,
            labels_json,
            author,
            assignees_json,
            is_pr,
        ))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        let (
            issue_ref,
            state,
            start_date,
            target_date,
            body,
            labels_json,
            author,
            assignees_json,
            is_pr,
        ) = row;
        map.insert(
            issue_ref,
            IssueDates {
                start_date,
                target_date,
                state: Some(state),
                description: if body.is_empty() { None } else { Some(body) },
                labels: serde_json::from_str(&labels_json).unwrap_or_default(),
                author: if author.is_empty() {
                    None
                } else {
                    Some(author)
                },
                assignees: serde_json::from_str(&assignees_json).unwrap_or_default(),
                is_pr,
            },
        );
    }
    map
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "windows")]
    let cmd = "cmd";

    #[cfg(target_os = "windows")]
    let args = vec!["/c", "start", url];
    #[cfg(not(target_os = "windows"))]
    let args = vec![url];

    let _ = std::process::Command::new(cmd).args(&args).spawn();
}
