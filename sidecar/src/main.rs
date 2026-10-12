//! `remotesmith-tray` — RemoteSmith presence tray / menu-bar app.
// GUI subsystem on Windows so a login (Run key) launch opens no console window. CLI modes
// re-attach to the console of the shell that started them; see `attach_parent_console`.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::time::Duration;

use remotesmith_tray::client::{self, ClientConfig, Command};
use remotesmith_tray::menu_model::{self, Item};
use remotesmith_tray::{autostart, logging, platform, protocol, tray};

const USAGE: &str = "\
remotesmith-tray — RemoteSmith presence tray

USAGE:
    remotesmith-tray [--endpoint <path>]     run the tray (default)
    remotesmith-tray --install-autostart     start at login (LaunchAgent / HKCU Run)
    remotesmith-tray --uninstall-autostart   stop starting at login
    remotesmith-tray --print-menu [--wait <secs>] [--endpoint <path>]
                                             connect headlessly and print the menu model
    remotesmith-tray --version               print version and IPC protocol range

ENV:
    REMOTESMITH_IPC_PATH   override the IPC endpoint (socket path / pipe name)
    REMOTESMITH_LOG        debug | trace | warn (default info)
";

fn print_items(items: &[Item], depth: usize) {
    let pad = "  ".repeat(depth);
    for i in items {
        match i {
            Item::Label(t) => println!("{pad}{t}"),
            Item::Action { text, enabled, .. } => {
                println!("{pad}[{}] {text}", if *enabled { "x" } else { " " })
            }
            Item::Submenu { text, items, .. } => {
                println!("{pad}{text} >");
                print_items(items, depth + 1);
            }
            Item::Separator => println!("{pad}----"),
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut cfg = ClientConfig::default();
    let mut mode = "run";
    let mut wait = 3u64;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--install-autostart" => mode = "install",
            "--uninstall-autostart" => mode = "uninstall",
            "--print-menu" => mode = "print",
            "--version" | "-V" => mode = "version",
            "--help" | "-h" => mode = "help",
            "--endpoint" => match args.next() {
                Some(p) => cfg.endpoint = p,
                None => die("--endpoint needs a value"),
            },
            "--wait" => match args.next().and_then(|s| s.parse().ok()) {
                Some(w) => wait = w,
                None => die("--wait needs a number of seconds"),
            },
            other => die(&format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    if mode != "run" {
        attach_parent_console();
    }
    match mode {
        "help" => print!("{USAGE}"),
        "version" => println!(
            "remotesmith-tray {} (IPC protocol {}–{})",
            env!("CARGO_PKG_VERSION"),
            protocol::MIN_VERSION,
            protocol::MAX_VERSION
        ),
        "install" => match autostart::install() {
            Ok(m) => println!("{m}"),
            Err(e) => die(&format!("install failed: {e}")),
        },
        "uninstall" => match autostart::uninstall() {
            Ok(m) => println!("{m}"),
            Err(e) => die(&format!("uninstall failed: {e}")),
        },
        "print" => {
            logging::init("tray-cli", false);
            let (tx, rx) = std::sync::mpsc::channel();
            let (cmds, h) = client::spawn(cfg, move |s| {
                let _ = tx.send(s.clone());
            });
            std::thread::sleep(Duration::from_secs(wait));
            let mut last = None;
            while let Ok(s) = rx.try_recv() {
                last = Some(s);
            }
            let _ = cmds.send(Command::Shutdown);
            let _ = h.join();
            let state = last.unwrap_or_default();
            let model = menu_model::build(&state, client::unix_now());
            println!("tooltip: {}", model.tooltip);
            println!("badge:   {}", model.badge.as_deref().unwrap_or("(none)"));
            print_items(&model.items, 0);
        }
        _ => {
            let log = logging::init("tray", std::env::var_os("REMOTESMITH_LOG_STDERR").is_some());
            let _lock = match platform::single_instance_lock() {
                Ok(Some(f)) => Some(f),
                Ok(None) => {
                    log::info!("another tray instance is running; exiting");
                    eprintln!("remotesmith-tray is already running");
                    return;
                }
                Err(e) => {
                    log::warn!("single-instance lock unavailable: {e}");
                    None
                }
            };
            log::info!(
                "remotesmith-tray {} starting; endpoint {}; log {:?}",
                env!("CARGO_PKG_VERSION"),
                cfg.endpoint,
                log
            );
            tray::run(cfg);
        }
    }
}

/// On Windows (GUI subsystem) borrow the parent shell's console so CLI output is visible.
/// No-op when there is no parent console (e.g. launched from Explorer) and on other OSes.
fn attach_parent_console() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // SAFETY: plain FFI call; failure (no parent console, already attached) is harmless.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn die(msg: &str) -> ! {
    attach_parent_console();
    eprintln!("remotesmith-tray: {msg}");
    std::process::exit(2);
}
