mod app;
mod commands;
mod config;
mod context;
mod git;
mod herdr;
mod keys;
mod machines;
mod markdown;
mod search;
mod state;
mod ui;

use anyhow::{bail, Context, Result};
use app::View;
use crossterm::event::{self, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::execute;
use herdr::Client;
use serde_json::json;
use std::time::Duration;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("open") => open(&args[1..]),
        Some("ui") | None => run_ui(view_arg(&args)),
        Some("exec") => commands::run::exec_deferred(std::path::Path::new(args.get(1).context("exec needs a plan path")?)),
        Some("runner") => runner(),
        Some("api") => api(&args[1..]),
        Some("--version") | Some("-V") => {
            println!("herdr-raycast {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => bail!("unknown subcommand {other}. Use: open [--view V] | ui [--view V] | api <method> [json] | runner"),
    }
}

fn view_arg(args: &[String]) -> Option<View> {
    let from_args = args.iter().position(|a| a == "--view").and_then(|i| args.get(i + 1)).and_then(|v| View::parse(v));
    from_args.or_else(|| std::env::var("RAYCAST_VIEW").ok().and_then(|v| View::parse(&v)))
}

fn open(args: &[String]) -> Result<()> {
    let client = Client::from_env();
    let cfg = config::load();
    if let Some(open) = state::open_palette() {
        state::clear_open();
        if open.pane_id == state::POPUP {
            if client.call("popup.close", json!({})).is_ok() {
                return Ok(());
            }
        } else if client.snapshot().map(|s| s.pane(&open.pane_id).is_some()).unwrap_or(false) {
            let _ = client.close_pane(&open.pane_id);
            return Ok(());
        }
    }
    let ctx = context::Captured::capture(&client);
    let view = view_arg(args).unwrap_or(View::All);
    let mut env = std::collections::HashMap::new();
    env.insert(context::CONTEXT_ENV.to_string(), ctx.to_env_json());
    env.insert("RAYCAST_VIEW".to_string(), view.title().to_lowercase());
    client.call("plugin.pane.open", json!({
        "plugin_id": commands::run::plugin_id(),
        "entrypoint": "palette",
        "placement": "popup",
        "width": cfg.width,
        "height": cfg.height,
        "cwd": ctx.dir_str(),
        "env": env,
        "focus": true,
    }))?;
    Ok(())
}

fn run_ui(view: Option<View>) -> Result<()> {
    let mut app = app::App::new(view)?;
    state::mark_open(&std::env::var("HERDR_PANE_ID").unwrap_or_else(|_| state::POPUP.into()));
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        hook(info);
    }));
    let mut terminal = ratatui::init();
    // Kitty encoding is what lets cmd/ctrl chords reach the palette unambiguously.
    let _ = execute!(std::io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    let debug_keys = app.cfg.debug_keys;
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|f| ui::draw(f, &app))?;
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        if debug_keys {
                            commands::run::log(&format!("key {k:?}"));
                        }
                        app.handle_key(k)
                    }
                    _ => {}
                }
            }
            app.tick();
            if app.quit {
                return Ok(());
            }
        }
    })();
    let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    ratatui::restore();
    state::clear_open();
    result
}

fn runner() -> Result<()> {
    let run = std::env::var("RAYCAST_RUN").unwrap_or_default();
    if run.is_empty() {
        bail!("RAYCAST_RUN is empty");
    }
    let status = std::process::Command::new("sh").arg("-c").arg(&run).status()?;
    if !status.success() {
        eprintln!("\n[{run}] exited with {status}. Press enter to close.");
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    }
    Ok(())
}

fn api(args: &[String]) -> Result<()> {
    let method = args.first().context("api needs a method")?;
    let params: serde_json::Value = match args.get(1) {
        Some(p) => serde_json::from_str(p).context("params must be json")?,
        None => json!({}),
    };
    match Client::from_env().call(method, params) {
        Ok(v) => {
            println!("{}", serde_json::to_string(&v)?);
            Ok(())
        }
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }
}
