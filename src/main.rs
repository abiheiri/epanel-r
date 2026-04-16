//! epanel
//!
//! A TUI panel for managing links and notes.
//!
//! Author: Al Biheiri <al@forgottheaddress.com>

use std::io;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};

mod app;
// Safari sync is only available on macOS.
// Keeping it out of the build on other OS
#[cfg(target_os = "macos")]
mod safari_sync;
mod ui;

use app::App;

#[derive(Parser)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(about = "A TUI epanel", disable_version_flag = true)]
struct Cli {
    /// Print version information
    #[arg(short = 'v', long)]
    version: bool,

    /// Update to the latest release
    #[arg(long)]
    update: bool,

    #[arg(long)]
    about: bool,
}

fn main() -> Result<()> {
    let cmd = Cli::command().version(app::VERSION);
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    if cli.version {
        println!("{} {}", env!("CARGO_PKG_NAME"), app::VERSION);
        return Ok(());
    }

    if cli.update {
        return update();
    }

    if cli.about {
        println!("Author: Al Biheiri <al@forgottheaddress.com>");
        println!("Website: http://www.abiheiri.com");
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.load()?;

    #[cfg(target_os = "macos")]
    let (sync_tx, sync_rx) = std::sync::mpsc::channel::<(Vec<app::Folder>, app::Folder)>();
    #[cfg(target_os = "macos")]
    {
        app.sync_tx = Some(sync_tx.clone());
        if app.safari_sync_enabled {
            app.sync_safari_on_startup();
            app.start_safari_sync();
        }
    }

    #[cfg(not(target_os = "macos"))]
    let sync_rx: std::sync::mpsc::Receiver<(Vec<app::Folder>, app::Folder)> = {
        let (_, rx) = std::sync::mpsc::channel();
        rx
    };

    let res = run_app(&mut terminal, &mut app, sync_rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    app.save()?;

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    #[allow(unused_variables)] sync_rx: std::sync::mpsc::Receiver<(Vec<app::Folder>, app::Folder)>,
) -> Result<()> {
    terminal.draw(|f| ui::draw(f, app))?;

    loop {
        let timeout = match app.next_timer_deadline() {
            Some(deadline) => {
                let now = std::time::Instant::now();
                if deadline <= now {
                    std::time::Duration::from_millis(0)
                } else {
                    // Cap at 200 ms so background Safari sync events are processed promptly.
                    // crossterm's event::poll only watches stdin, so we can't be woken by
                    // the sync channel; putting a short periodic poll keeps CPU near zero while
                    // remaining responsive.
                    (deadline - now).min(std::time::Duration::from_millis(200))
                }
            }
            None => std::time::Duration::from_millis(200),
        };

        let mut changed = false;

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if app.handle_key(key.code, key.modifiers) {
                            return Ok(());
                        }
                        changed = true;
                    }
                }
                Event::Resize(_, _) => changed = true,
                _ => {}
            }
        }

        if app.check_timers() {
            changed = true;
        }

        #[cfg(target_os = "macos")]
        if app.popup.is_none() {
            if let Ok((folders, reading_list)) = sync_rx.try_recv() {
                let should_apply = std::fs::metadata(app::expand_tilde(&app.safari_sync_path))
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|modified| {
                        app.last_safari_writeback.map(|last| modified > last).unwrap_or(true)
                    })
                    .unwrap_or(true);
                if should_apply {
                    app.apply_safari_sync(folders, reading_list);
                    changed = true;
                }
            }
        } else {
            // Drain sync updates while a popup is open to avoid backlog
            while let Ok(_) = sync_rx.try_recv() {}
        }

        if changed {
            terminal.draw(|f| ui::draw(f, app))?;
        }
    }
}

// Check GitHub for the latest release and update if a newer version is available.
fn update() -> Result<()> {
    let repo_url = env!("CARGO_PKG_REPOSITORY");
    let parts: Vec<&str> = repo_url.trim_end_matches('/').split('/').collect();
    if parts.len() < 2 {
        anyhow::bail!("Invalid repository URL in Cargo.toml: {}", repo_url);
    }
    let owner = parts[parts.len() - 2];
    let repo = parts[parts.len() - 1];

    let target = if cfg!(target_os = "macos") {
        format!("{}-apple-darwin", std::env::consts::ARCH)
    } else if cfg!(target_os = "linux") {
        format!("{}-unknown-linux-gnu", std::env::consts::ARCH)
    } else {
        anyhow::bail!("Uh? Unsupported platform");
    };

    let api_url = format!("https://api.github.com/repos/{}/{}/releases/latest", owner, repo);
    println!("Checking for latest release...");
    let mut resp = match ureq::get(&api_url)
        .header("User-Agent", concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .call()
    {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(404)) => {
            println!("No updates available.");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let body = resp.body_mut().read_to_string()?;
    let asset_name = format!("{}-{}.tar.gz", env!("CARGO_PKG_NAME"), target);

    let url = serde_json::from_str::<serde_json::Value>(&body)?
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|asset| {
                let name = asset.get("name")?.as_str()?;
                if name == asset_name {
                    asset.get("browser_download_url")?.as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Could not find asset {} in latest release", asset_name))?;

    println!("Downloading update from {} ...", url);
    let mut dl_resp = ureq::get(&url)
        .header("User-Agent", concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let mut reader = dl_resp.body_mut().as_reader();

    let tmp_dir = std::env::temp_dir().join(format!("{}-update", env!("CARGO_PKG_NAME")));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir)?;

    let tar_path = tmp_dir.join("update.tar.gz");
    let mut file = std::fs::File::create(&tar_path)?;
    std::io::copy(&mut reader, &mut file)?;
    drop(file);

    let tar_gz = std::fs::File::open(&tar_path)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    archive.unpack(&tmp_dir)?;

    let current_exe = std::env::current_exe()?;
    let new_exe = tmp_dir.join(env!("CARGO_PKG_NAME"));

    if !new_exe.exists() {
        anyhow::bail!("Update archive did not contain expected binary");
    }

    let backup = current_exe.with_extension("old");
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(&current_exe, &backup)?;
    std::fs::copy(&new_exe, &current_exe)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&current_exe)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&current_exe, perms)?;
    }
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::remove_dir_all(&tmp_dir);

    println!("Updated to the latest version successfully.");
    Ok(())
}
