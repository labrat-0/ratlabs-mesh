//! `ratmesh mcp`: MCP over stdio for clients that launch their servers as a command.
//!
//! Every message is forwarded to the studio's /mcp endpoint, so jobs an AI starts run in
//! the studio (one queue, one worker) and show up live in the user's window. The studio is
//! started in the background if it isn't running.
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use mesh_core::config;
use serde_json::{Value, json};

fn up() -> bool {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .ok()
        .and_then(|c| c.get(format!("{}api/config", config().url())).send().ok())
        .is_some_and(|r| r.status().is_success())
}

/// Start `ratmesh serve` detached (logging to the cache folder) and wait for it.
pub fn ensure_server() -> Result<()> {
    if up() {
        return Ok(());
    }
    let log = config().cache_dir.join("studio.log");
    std::fs::create_dir_all(log.parent().unwrap())?;
    let out = std::fs::File::create(&log)?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("serve").stdin(Stdio::null()).stdout(out.try_clone()?).stderr(out);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // outlive the AI client's process group
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS); // no console window, outlives the caller
    }
    cmd.spawn()?;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        if up() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    bail!("the studio did not start; see {}", log.display())
}

pub fn run() -> Result<()> {
    ensure_server()?;
    let url = format!("{}mcp", config().url());
    // Tool calls may wait up to 10 minutes for a job.
    let http = reqwest::blocking::Client::builder().timeout(Duration::from_secs(900)).build()?;
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let session: Arc<Mutex<Option<String>>> = Arc::default();
    let client: Arc<Mutex<String>> = Arc::new(Mutex::new("AI client".into()));
    let write = |out: &Arc<Mutex<std::io::Stdout>>, v: &Value| {
        let mut o = out.lock().unwrap();
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    };

    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                write(&out, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}));
                continue;
            }
        };
        let is_init = msg["method"] == "initialize";
        if let Some(name) = mesh_mcp::client_name(&msg) {
            *client.lock().unwrap() = name;
        }
        let (http, url, out, session, client) = (http.clone(), url.clone(), out.clone(), session.clone(), client.clone());
        let send = move || {
            let mut req =
                http.post(&url).header("Accept", "application/json, text/event-stream").header("X-Ratmesh-Client", client.lock().unwrap().clone()).json(&msg);
            if let Some(s) = session.lock().unwrap().clone() {
                req = req.header("Mcp-Session-Id", s);
            }
            let reply = req.send().or_else(|_| {
                // The studio went away (closed or restarted): bring it back once and retry.
                ensure_server().map_err(|e| e.to_string()).ok();
                let mut r = http.post(&url).header("X-Ratmesh-Client", client.lock().unwrap().clone()).json(&msg);
                if let Some(s) = session.lock().unwrap().clone() {
                    r = r.header("Mcp-Session-Id", s);
                }
                r.send()
            });
            match reply {
                Ok(r) => {
                    if let Some(s) = r.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
                        *session.lock().unwrap() = Some(s.to_string());
                    }
                    if r.status() == reqwest::StatusCode::ACCEPTED {
                        return;
                    }
                    match r.json::<Value>() {
                        Ok(v) => write(&out, &v),
                        Err(e) => {
                            if let Some(id) = msg.get("id") {
                                write(
                                    &out,
                                    &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": format!("bad reply from the studio: {e}")}}),
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Some(id) = msg.get("id") {
                        write(
                            &out,
                            &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": format!("RatLabs Mesh studio unreachable: {e}")}}),
                        );
                    }
                }
            }
        };
        if is_init {
            send(); // the session id must exist before anything else goes out
        } else {
            std::thread::spawn(send);
        }
    }
    Ok(())
}
