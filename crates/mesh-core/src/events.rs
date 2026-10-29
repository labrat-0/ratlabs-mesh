//! One broadcast bus for everything that changes. The studio subscribes over SSE, so a
//! job started by an AI client shows up and updates live without anyone refreshing.
use std::sync::OnceLock;

use serde::Serialize;
use tokio::sync::broadcast;

use crate::jobs::Job;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A job was created or its status, stage or results changed.
    Job { job: Box<Job> },
    /// Ask every open studio to show this job (an AI client wants the user to look).
    Show { id: String, variant: Option<u32>, mode: Option<String>, light: Option<String> },
    /// An AI client connected or called a tool; the studio shows who is driving.
    Agent { client: String, tool: String },
    /// Keys or engines changed; reload the config.
    Config,
    /// The Market library changed.
    Library,
}

fn bus() -> &'static broadcast::Sender<Event> {
    static BUS: OnceLock<broadcast::Sender<Event>> = OnceLock::new();
    BUS.get_or_init(|| broadcast::channel(256).0)
}

/// Send to every listener. Nobody listening (e.g. the CLI) is fine.
pub fn emit(ev: Event) {
    let _ = bus().send(ev);
}

pub fn subscribe() -> broadcast::Receiver<Event> {
    bus().subscribe()
}
