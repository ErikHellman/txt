mod app;
mod buffer;
mod clipboard;
mod config;
mod editor;
mod error;
mod git;
mod input;
mod lsp;
mod search;
mod syntax;
mod theme;
mod ui;
mod watcher;

use std::{io, path::PathBuf};

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{app::App, editor::Editor};

/// A fast, intuitive terminal text editor.
///
/// Press F1 inside the editor for the full key binding list.
#[derive(Parser)]
#[command(name = "txt", version, about, long_about = None)]
struct Cli {
    /// File or directory to open.
    path: Option<PathBuf>,

    /// Print shell completion script and exit.
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "txt", &mut io::stdout());
        return Ok(());
    }

    let (editor, open_sidebar) = match cli.path {
        Some(p) if p.is_dir() => {
            std::env::set_current_dir(&p)?;
            (Editor::new(), true)
        }
        Some(p) => (Editor::open(p)?, false),
        None => (Editor::new(), false),
    };
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Install a panic hook that restores the terminal before printing the panic.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info);
    }));

    let terminal = init_terminal()?;
    let result = App::new().run(terminal, editor, open_sidebar, workspace);
    restore_terminal()?;

    if let Err(e) = result {
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }

    Ok(())
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    // Enable keyboard enhancements so Ctrl+digit and Ctrl+Tab are transmitted
    // correctly by terminals that support the protocol (kitty, WezTerm, foot,
    // recent iTerm2). Terminals that don't support it silently ignore this.
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal() -> Result<()> {
    let mut stdout = io::stdout();
    // Disable terminal features while still in raw mode.
    // Calling disable_raw_mode() first leaves a window where mouse tracking
    // is still active but the tty is in cooked/echo mode — any mouse motion
    // during that window produces SGR sequences that land in the shell's
    // stdin and get printed as literal text.
    // Separate execute! calls ensure DisableMouseCapture is always sent
    // even if PopKeyboardEnhancementFlags fails on an unsupported terminal.
    let _ = execute!(stdout, PopKeyboardEnhancementFlags);
    let _ = execute!(stdout, DisableMouseCapture);
    let _ = execute!(stdout, LeaveAlternateScreen);
    disable_raw_mode()?;
    Ok(())
}
