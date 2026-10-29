//! ratmesh: RatLabs Mesh from the command line.
//!
//!   ratmesh serve                                       # the studio, http://127.0.0.1:8771
//!   ratmesh mcp                                         # MCP over stdio, for AI clients
//!   ratmesh make art.png --kind character --engine trellis2 --variants 2
//!   ratmesh make scene.png --crop 0.3,0.1,0.7,1.0       # pick a character out of a scene
//!   ratmesh make front.png --back back.png --engine hunyuan3
//!   ratmesh import old.glb --kind critter --height 0.1  # finish an existing model
//!   ratmesh refinish <job> --polys 12000 --height 1.4   # re-finish, no GPU cost
//!   ratmesh rig <job>                                   # skeleton + 88 CC0 animations
//!   ratmesh engines | doctor | connect
use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use mesh_core::{Job, NewJob, config};

mod desktop;
mod mcp_stdio;
mod setup;

#[derive(Parser)]
#[command(name = "ratmesh", version, about = "RatLabs Mesh: image to finished, game-ready 3D asset", propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone)]
struct Shape {
    #[arg(long)]
    name: Option<String>,
    #[arg(long, default_value = "character", value_parser = kinds())]
    kind: String,
    /// Metres, up axis
    #[arg(long)]
    height: Option<f64>,
    /// Triangle budget
    #[arg(long)]
    polys: Option<u32>,
    #[arg(long, value_parser = ["1024", "2048", "4096"])]
    texture: Option<String>,
    #[arg(long, value_parser = ["bottom", "center"])]
    origin: Option<String>,
    /// Degrees about up, if the model faces the wrong way
    #[arg(long, default_value_t = 0.0)]
    rotate: f64,
}

fn kinds() -> Vec<&'static str> {
    mesh_core::PRESETS.iter().map(|p| p.kind).collect()
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the studio UI (and the MCP endpoint at /mcp)
    Serve {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Speak MCP over stdio for AI clients; starts the studio if it isn't running
    Mcp,
    /// Generate from an image
    Make {
        image: PathBuf,
        #[command(flatten)]
        shape: Shape,
        #[arg(long, default_value = "trellis2")]
        engine: String,
        /// 1-4 seeds to pick from
        #[arg(long, default_value_t = 1)]
        variants: u32,
        #[arg(long, default_value = "standard", value_parser = ["draft", "standard", "high"])]
        quality: String,
        /// x0,y0,x1,y1 as 0-1 fractions
        #[arg(long)]
        crop: Option<String>,
        #[arg(long)]
        back: Option<PathBuf>,
        #[arg(long)]
        left: Option<PathBuf>,
        #[arg(long)]
        right: Option<PathBuf>,
        /// Meshy only
        #[arg(long, value_parser = ["a-pose", "t-pose"])]
        pose: Option<String>,
        #[arg(long)]
        no_pbr: bool,
        /// Skip cut-out (art already isolated)
        #[arg(long)]
        keep_background: bool,
        /// Paint with the art's own lighting (no delight)
        #[arg(long)]
        keep_lighting: bool,
        #[arg(long)]
        seed: Option<i64>,
    },
    /// Finish an existing GLB
    Import {
        glb: PathBuf,
        #[command(flatten)]
        shape: Shape,
    },
    /// Re-run finishing on a job (no GPU cost)
    Refinish {
        job: String,
        #[arg(long)]
        height: Option<f64>,
        #[arg(long)]
        polys: Option<u32>,
        #[arg(long)]
        texture: Option<u32>,
        #[arg(long)]
        rotate: Option<f64>,
        #[arg(long, value_parser = ["bottom", "center"])]
        origin: Option<String>,
    },
    /// Auto-rig a finished job and add the CC0 animation library
    Rig { job: String },
    /// List engines and whether they're ready
    Engines,
    /// Check Blender, engines, models and folders, with fixes for anything missing
    Doctor,
    /// Print MCP setup for Claude, ChatGPT/Codex, Grok, Cursor, VS Code, Gemini CLI and others
    Connect,
    /// Make source art from a description (fal.ai Nano Banana, ~$0.04)
    Concept {
        description: String,
        /// Style reference images
        #[arg(long = "ref")]
        refs: Vec<PathBuf>,
        #[arg(long)]
        style: Option<String>,
        #[arg(long)]
        seed: Option<i64>,
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// GLB tools that need no Blender
    #[command(subcommand)]
    Glb(GlbCmd),
    /// Pack a downloaded Market material into color / normal / ARM textures
    Matpack { source: String, id: String, out: PathBuf },
    /// Install optional parts: local engines, animations, the cut-out model
    #[command(subcommand)]
    Setup(setup::SetupCmd),
    /// Add RatLabs Mesh to the app launcher with a background service (Linux)
    InstallDesktop {
        #[arg(long)]
        uninstall: bool,
    },
}

#[derive(Subcommand)]
enum GlbCmd {
    /// Shrink embedded textures for the web (geometry untouched)
    Shrink {
        files: Vec<PathBuf>,
        #[arg(long, default_value_t = 1024)]
        max: u32,
        #[arg(long, default_value_t = 82)]
        quality: u8,
    },
    /// Scrub red/yellow cut-out halos from base-colour textures
    Defringe { files: Vec<PathBuf> },
    /// Triangle count
    Tris { files: Vec<PathBuf> },
    /// A lighter copy keeping skins and clips (Blender)
    Lod {
        input: PathBuf,
        out: PathBuf,
        /// Total triangles
        #[arg(long)]
        target: Option<u64>,
        /// Per-mesh share kept, 0-1
        #[arg(long)]
        ratio: Option<f64>,
    },
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn print_results(job: &Job) -> Result<()> {
    let d = job.dir();
    for r in &job.results {
        let s = &r.stats;
        let dims = s["dimensions_m"]
            .as_array()
            .map(|a| a.iter().map(|v| format!("{:.2}", v.as_f64().unwrap_or(0.0))).collect::<Vec<_>>().join(" x "))
            .unwrap_or_default();
        println!("  v{}: {}  {} tris  {dims} m  sheet {}", r.variant, d.join(&r.glb).display(), s["tris"], d.join(&r.renders).join("sheet.jpg").display());
        if let Some(rg) = &r.rigged {
            let clips = r.rig.as_ref().and_then(|g| g["clips"].as_array().map(|c| c.len())).unwrap_or(0);
            println!("       rigged: {}  {} clips", d.join(rg).display(), clips);
        }
    }
    if let Some(e) = &job.error {
        bail!("{e}");
    }
    Ok(())
}

fn shape_opts(s: &Shape) -> NewJob {
    NewJob {
        name: s.name.clone(),
        kind: Some(s.kind.clone()),
        height: s.height,
        polys: s.polys,
        texture: s.texture.as_ref().and_then(|t| t.parse().ok()),
        origin: s.origin.clone(),
        rotate: Some(s.rotate),
        by: Some("cli".into()),
        ..Default::default()
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Serve { port } => {
            if let Some(p) = port {
                // Before config() first loads, so the Host guard and links use this port too.
                // SAFETY: still single-threaded here; no other thread reads the environment.
                unsafe { std::env::set_var("RATMESH_PORT", p.to_string()) };
            }
            tokio::runtime::Runtime::new()?.block_on(mesh_server::serve(config().port))
        }
        Cmd::Mcp => mcp_stdio::run(),
        Cmd::Make { image, shape, engine, variants, quality, crop, back, left, right, pose, no_pbr, keep_background, keep_lighting, seed } => {
            if mesh_engines::get(&engine).is_none() {
                bail!("unknown engine {engine}; see `ratmesh engines`");
            }
            let crop = match crop {
                Some(c) => {
                    let v: Vec<f64> = c.split(',').map(|x| x.trim().parse()).collect::<Result<_, _>>()?;
                    if v.len() != 4 {
                        bail!("--crop needs x0,y0,x1,y1");
                    }
                    Some([v[0], v[1], v[2], v[3]])
                }
                None => None,
            };
            let views: BTreeMap<String, PathBuf> =
                [("back", back), ("left", left), ("right", right)].into_iter().filter_map(|(k, v)| v.map(|v| (k.to_string(), v))).collect();
            let opt = NewJob {
                engine: Some(engine),
                variants: Some(variants),
                quality: Some(quality),
                crop,
                pose,
                pbr: Some(!no_pbr),
                seed,
                keep_background: Some(keep_background),
                delight: Some(!keep_lighting),
                ..shape_opts(&shape)
            };
            let job = mesh_core::jobs::create(Some(&image), None, &views, opt)?;
            println!("job {}", job.id);
            print_results(&mesh_pipeline::run(&job.id)?)
        }
        Cmd::Import { glb, shape } => {
            let job = mesh_core::jobs::create(None, Some(&glb), &BTreeMap::new(), shape_opts(&shape))?;
            println!("job {}", job.id);
            print_results(&mesh_pipeline::run(&job.id)?)
        }
        Cmd::Refinish { job, height, polys, texture, rotate, origin } => {
            let j = mesh_pipeline::refinish(&job, &mesh_pipeline::Refinish { height, polys, texture, rotate, origin })?;
            println!("{}", serde_json::json!({"id": j.id, "status": j.status, "error": j.error}));
            Ok(())
        }
        Cmd::Rig { job } => print_results(&mesh_pipeline::rig(&job)?),
        Cmd::Engines => {
            for (id, e) in mesh_engines::available().as_object().unwrap() {
                let ready = e["ready"] == true;
                let need = if ready { String::new() } else { format!("   {}", e["setup"].as_str().unwrap_or("not set up")) };
                println!(
                    "{}  {id:14} {:30} {}{need}",
                    if ready { "ready " } else { "  --  " },
                    e["label"].as_str().unwrap_or(""),
                    e["cost"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        Cmd::Doctor => setup::doctor(),
        Cmd::Connect => {
            let info = mesh_server_connect();
            println!(
                "MCP endpoint: {}\nstdio command: {}\n",
                info["url"].as_str().unwrap(),
                info["command"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>().join(" ")
            );
            for c in info["clients"].as_array().unwrap() {
                println!("── {} ({})\n{}\n", c["name"].as_str().unwrap(), c["where"].as_str().unwrap(), c["text"].as_str().unwrap());
            }
            println!("The stdio command starts the studio if it isn't running. The HTTP URL needs `ratmesh serve` (or install-desktop).");
            Ok(())
        }
        Cmd::Concept { description, refs, style, seed, out } => {
            let out = out.unwrap_or_else(|| PathBuf::from(format!("{}.png", mesh_core::slug(&description))));
            let refs: Vec<&std::path::Path> = refs.iter().map(|p| p.as_path()).collect();
            mesh_engines::concept::concept(&description, &out, &refs, style.as_deref(), seed, &|m| eprintln!("{m}"))?;
            println!("{}", out.display());
            Ok(())
        }
        Cmd::Glb(g) => match g {
            GlbCmd::Shrink { files, max, quality } => {
                for f in files {
                    let (a, b) = mesh_glb::shrink(&f, max, quality)?;
                    println!("{}: {:.1} MB -> {:.1} MB", f.display(), a as f64 / 1e6, b as f64 / 1e6);
                }
                Ok(())
            }
            GlbCmd::Defringe { files } => {
                for f in files {
                    println!("{} {} fringe pixels replaced", f.display(), mesh_glb::defringe(&f)?);
                }
                Ok(())
            }
            GlbCmd::Tris { files } => {
                for f in files {
                    println!("{} {}", f.display(), mesh_glb::tris(&f));
                }
                Ok(())
            }
            GlbCmd::Lod { input, out, target, ratio } => {
                let mut spec = serde_json::json!({});
                if let Some(t) = target {
                    spec["target"] = t.into();
                }
                if let Some(r) = ratio {
                    spec["ratio"] = r.into();
                }
                println!("{}", mesh_blender::lod(&input, &out, spec)?);
                Ok(())
            }
        },
        Cmd::Matpack { source, id, out } => {
            println!("{}", serde_json::to_string_pretty(&mesh_market::matpack::pack(&source, &id, &out)?)?);
            Ok(())
        }
        Cmd::Setup(s) => setup::run(s),
        Cmd::InstallDesktop { uninstall } => desktop::run(uninstall),
    }
}

fn mesh_server_connect() -> serde_json::Value {
    // The same snippets the studio's "Connect AI" dialog shows.
    let url = format!("{}api/connect", config().url());
    reqwest::blocking::get(&url).and_then(|r| r.json()).unwrap_or_else(|_| {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "ratmesh".into());
        let mcp = format!("{}mcp", config().url());
        serde_json::json!({
            "url": mcp, "command": [exe, "mcp"],
            "clients": [
                {"name": "Claude Code", "where": "terminal", "text": format!("claude mcp add ratlabs-mesh -- {exe} mcp")},
                {"name": "Any MCP client (stdio)", "where": "its MCP config", "text": serde_json::to_string_pretty(&serde_json::json!({"mcpServers": {"ratlabs-mesh": {"command": exe, "args": ["mcp"]}}})).unwrap()},
            ],
        })
    })
}
