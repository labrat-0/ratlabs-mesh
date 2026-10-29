//! "Connect an AI": ready-to-paste MCP setup for the common clients, using this binary.
use serde_json::{Value, json};

pub fn info() -> Value {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "ratmesh".into());
    let url = format!("{}mcp", mesh_core::config().url());
    let stdio = json!({"mcpServers": {"ratlabs-mesh": {"command": exe, "args": ["mcp"]}}});
    json!({
        "url": url,
        "command": [exe, "mcp"],
        "clients": [
            {"name": "Claude Code", "where": "terminal", "text": format!("claude mcp add --transport http ratlabs-mesh {url}")},
            {"name": "Claude Desktop", "where": "Settings > Developer > Edit Config (claude_desktop_config.json)", "text": serde_json::to_string_pretty(&stdio).unwrap()},
            {"name": "Cursor", "where": "~/.cursor/mcp.json", "text": serde_json::to_string_pretty(&json!({"mcpServers": {"ratlabs-mesh": {"url": url}}})).unwrap()},
            {"name": "VS Code (Copilot)", "where": ".vscode/mcp.json", "text": serde_json::to_string_pretty(&json!({"servers": {"ratlabs-mesh": {"type": "http", "url": url}}})).unwrap()},
            {"name": "Codex CLI", "where": "~/.codex/config.toml", "text": format!("[mcp_servers.ratlabs-mesh]\ncommand = {}\nargs = [\"mcp\"]", toml_str(&exe))},
            {"name": "Gemini CLI", "where": "~/.gemini/settings.json", "text": serde_json::to_string_pretty(&json!({"mcpServers": {"ratlabs-mesh": {"httpUrl": url}}})).unwrap()},
            {"name": "Grok (Grok Build CLI)", "where": "terminal", "text": format!("grok mcp add ratlabs-mesh {url}")},
            {"name": "LM Studio, Windsurf, Zed, others", "where": "their MCP config (stdio)", "text": serde_json::to_string_pretty(&stdio).unwrap()},
        ],
    })
}

fn toml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
