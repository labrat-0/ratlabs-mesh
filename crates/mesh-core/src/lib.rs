//! RatLabs Mesh core: where things live, what a finished asset should look like,
//! the job store, and the event bus the studio and AI clients listen to.

pub mod config;
pub mod events;
pub mod jobs;
pub mod keys;
pub mod presets;

pub use config::{Config, config};
pub use events::{Event, emit, subscribe};
pub use jobs::{Job, JobResult, NewJob};
pub use presets::{PRESETS, Preset, preset};

/// Lowercase ASCII words joined by dashes, at most 40 characters.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(40).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "asset".into() } else { out }
}

/// Seconds since the epoch, as a float (what job.json has always stored).
pub fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn slugs() {
        assert_eq!(slug("Goblin Intern (v2)"), "goblin-intern-v2");
        assert_eq!(slug("!!!"), "asset");
        assert_eq!(slug("a".repeat(60).as_str()).len(), 40);
    }
}
