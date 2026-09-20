use std::{
    fs::OpenOptions,
    io,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use clap::CommandFactory;
use clap_complete::{Shell, generate};

const MARKER: &str = "# txt shell completions";

/// Resolve the shell from a CLI argument ("auto" reads `$SHELL`) and install
/// completions. Shells without an rc-file convention we want to guess at get
/// their generated script printed with an instruction instead.
pub fn install(shell_arg: &str) -> Result<()> {
    let shell = resolve_shell(shell_arg)?;
    match rc_path(shell) {
        Some(rc) => {
            let installed = append_snippet(&rc, shell)?;
            if installed {
                println!("Installed txt completions for {shell} in {}", rc.display());
            } else {
                println!("txt completions already installed in {}", rc.display());
            }
            Ok(())
        }
        None => {
            println!("# Add the following to your {shell} profile to enable txt completions:");
            let mut cmd = crate::Cli::command();
            generate(shell, &mut cmd, "txt", &mut io::stdout());
            Ok(())
        }
    }
}

fn resolve_shell(shell_arg: &str) -> Result<Shell> {
    if shell_arg == "auto" {
        let shell_path = std::env::var("SHELL").context(
            "cannot detect the current shell ($SHELL is not set); \
             specify one, e.g. txt --install-completions bash",
        )?;
        let name = shell_path.rsplit('/').next().unwrap_or(&shell_path);
        parse_shell(name)
    } else {
        parse_shell(shell_arg)
    }
}

fn parse_shell(name: &str) -> Result<Shell> {
    name.parse()
        .map_err(|e: String| anyhow!("unknown shell '{name}': {e}"))
}

/// The startup file the completion snippet belongs in, or `None` if there is
/// no convention we are willing to guess at.
fn rc_path(shell: Shell) -> Option<PathBuf> {
    match shell {
        Shell::Bash => dirs::home_dir().map(|h| h.join(".bashrc")),
        Shell::Zsh => match std::env::var_os("ZDOTDIR") {
            Some(zd) => Some(PathBuf::from(zd).join(".zshrc")),
            None => dirs::home_dir().map(|h| h.join(".zshrc")),
        },
        Shell::Fish => dirs::config_dir().map(|c| c.join("fish").join("config.fish")),
        _ => None,
    }
}

/// Append the marker and snippet to `rc`, creating the file if needed.
/// Returns `Ok(false)` when the marker is already present (no-op).
fn append_snippet(rc: &Path, shell: Shell) -> Result<bool> {
    if let Ok(existing) = std::fs::read_to_string(rc)
        && existing.lines().any(|l| l.trim() == MARKER)
    {
        return Ok(false);
    }
    if let Some(parent) = rc.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(rc)
        .with_context(|| format!("cannot write to {}", rc.display()))?;
    writeln!(f)?;
    writeln!(f, "{MARKER}")?;
    writeln!(f, "{}", snippet_line(shell).unwrap())?;
    Ok(true)
}

fn snippet_line(shell: Shell) -> Option<String> {
    match shell {
        Shell::Bash => Some("source <(txt --completions bash)".into()),
        Shell::Zsh => Some("source <(txt --completions zsh)".into()),
        Shell::Fish => Some("txt --completions fish | source".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shell_accepts_known_names() {
        assert_eq!(parse_shell("bash").unwrap(), Shell::Bash);
        assert!(parse_shell("zsh").unwrap() != Shell::Bash);
        assert!(matches!(parse_shell("fish"), Ok(Shell::Fish)));
    }

    #[test]
    fn parse_shell_rejects_unknown() {
        assert!(parse_shell("tcsh").is_err());
        assert!(parse_shell("").is_err());
    }

    #[test]
    fn resolve_shell_reads_shebang_from_env() {
        // SAFETY: single-threaded test; no other thread reads SHELL.
        unsafe { std::env::set_var("SHELL", "/bin/zsh") };
        assert_eq!(resolve_shell("auto").unwrap(), Shell::Zsh);
        assert_eq!(resolve_shell("bash").unwrap(), Shell::Bash);
    }

    #[test]
    fn snippet_lines_cover_rc_shells() {
        assert_eq!(
            snippet_line(Shell::Bash).unwrap(),
            "source <(txt --completions bash)"
        );
        assert_eq!(
            snippet_line(Shell::Fish).unwrap(),
            "txt --completions fish | source"
        );
    }

    #[test]
    fn append_creates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        assert!(append_snippet(&rc, Shell::Bash).unwrap());
        let content = std::fs::read_to_string(&rc).unwrap();
        assert!(content.contains(MARKER));
        assert!(content.contains("source <(txt --completions bash)"));
    }

    #[test]
    fn append_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        assert!(append_snippet(&rc, Shell::Bash).unwrap());
        assert!(!append_snippet(&rc, Shell::Bash).unwrap());
        let content = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(content.matches(MARKER).count(), 1);
    }

    #[test]
    fn append_detects_marker_with_surrounding_text() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        std::fs::write(&rc, format!("export EDITOR=txt\n  {MARKER}  \n")).unwrap();
        assert!(!append_snippet(&rc, Shell::Zsh).unwrap());
    }
}
