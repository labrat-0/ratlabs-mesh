//! Model Context Protocol for RatLabs Mesh. Any MCP client (Claude, ChatGPT, Cursor,
//! Gemini CLI, LM Studio, a local model...) gets the studio's abilities as tools, while the
//! person watches every step happen in the studio window.
//!
//! Transport-free: `handle` takes one JSON-RPC message and returns the reply. The server
//! mounts it at POST /mcp (streamable HTTP); `ratmesh mcp` bridges stdio to that endpoint.
use serde_json::{Value, json};

mod input;
mod tools;

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "RatLabs Mesh turns 2D art into finished, game-ready 3D models (GLB) on the user's machine, \
and the user watches each step live in the RatLabs Mesh studio.\n\
Typical flow: mesh_status (see which engines are ready) -> preview_cutout (check the subject was isolated) -> \
generate_model -> get_job with wait_seconds until done -> view_model to look at the review renders -> \
refinish_model / rig_model as needed. You see models only through view_model's renders; show_in_studio \
(texture/clay/wire, studio/neon lighting) changes what the user sees. concept_art makes source art from a description first; the Market tools find free assets.\n\
Costs: engines whose `local` is false bill the user's own account (about $0.25-0.38 per model per variant on fal.ai). \
Ask before using a paid engine, or more than one variant, unless the user already said to. Local engines are free but take ~25 minutes.\n\
Best results come from four views of the same character: front (the image) plus back, left and right \
(generate_model's back/left/right, used by multiview engines such as hunyuan3 and meshy). If the user only has a front, \
suggest making the other three with any free AI image generator: same character, same outfit, A-pose, plain background, \
whole body in frame, one image per view.\n\
Paths in results are absolute paths on the user's machine; images can be given as a path, an http(s) URL or a data: URI.";

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The client's name from an `initialize` request, if this is one.
pub fn client_name(msg: &Value) -> Option<String> {
    (msg["method"] == "initialize").then(|| {
        let ci = &msg["params"]["clientInfo"];
        ci["title"].as_str().or(ci["name"].as_str()).unwrap_or("AI client").to_string()
    })
}

/// Handle one JSON-RPC message (or a batch). Notifications get no reply.
pub fn handle(msg: &Value, client: &str) -> Option<Value> {
    if let Some(batch) = msg.as_array() {
        let replies: Vec<Value> = batch.iter().filter_map(|m| handle(m, client)).collect();
        return (!replies.is_empty()).then(|| Value::Array(replies));
    }
    let id = msg.get("id")?.clone(); // no id: a notification
    let method = msg["method"].as_str().unwrap_or("");
    let params = &msg["params"];
    Some(match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = if PROTOCOL_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSIONS[0] };
            mesh_core::emit(mesh_core::Event::Agent { client: client.into(), tool: "connected".into() });
            ok(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "ratlabs-mesh", "title": "RatLabs Mesh", "version": env!("CARGO_PKG_VERSION"), "websiteUrl": "https://ratlabs.tech"},
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => ok(&id, json!({})),
        "tools/list" => ok(&id, json!({"tools": tools::list()})),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !tools::exists(name) {
                return Some(error(&id, -32602, &format!("unknown tool {name}")));
            }
            mesh_core::emit(mesh_core::Event::Agent { client: client.into(), tool: name.into() });
            ok(&id, tools::call(name, &args, client))
        }
        "resources/list" => ok(&id, json!({"resources": []})),
        "prompts/list" => ok(&id, json!({"prompts": []})),
        _ => error(&id, -32601, &format!("method not found: {method}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_list() {
        let r =
            handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "t"}}}), "t")
                .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "ratlabs-mesh");
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), "t").is_none());
        let l = handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}), "t").unwrap();
        let tools = l["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "generate_model"));
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
            assert!(t["description"].as_str().unwrap().len() > 20);
        }
        let bad = handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "nope"}}), "t").unwrap();
        assert_eq!(bad["error"]["code"], -32602);
    }

    #[test]
    fn tool_errors_are_results() {
        let r =
            handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "get_job", "arguments": {"job_id": "../etc"}}}), "t").unwrap();
        assert_eq!(r["result"]["isError"], true);
    }
}
