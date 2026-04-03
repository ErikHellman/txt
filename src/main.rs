mod app;
mod buffer;
mod clipboard;
mod config;
mod editor;
mod error;
mod git;
mod input;
mod search;
mod syntax;
mod ui;
mod watcher;

use std::{io, path::PathBuf};

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{app::App, editor::Editor};

fn main() -> Result<()> {
    let arg = std::env::args().nth(1).map(PathBuf::from);

    let (editor, open_sidebar) = match arg {
        Some(p) if p.is_dir() => {
            std::env::set_current_dir(&p)?;
            (Editor::new(), true)
        }
        Some(p) => (Editor::open(p)?, false),
        None => (Editor::new(), false),
    };

    // Install a panic hook that restores the terminal before printing the panic.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original_hook(info);
    }));

    let terminal = init_terminal()?;
    let result = App::new().run(terminal, editor, open_sidebar);
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
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        PopKeyboardEnhancementFlags,
        DisableMouseCapture,
        LeaveAlternateScreen,
    )?;
    Ok(())
}
