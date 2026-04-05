use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::action::{EditorAction, action_from_name, action_to_name};
use crate::config::KeymapPreset;

// ── KeyCodeRepr ──────────────────────────────────────────────────────────────

/// Serializable subset of `crossterm::event::KeyCode`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCodeRepr {
    Char(char), // always lowercase
    F(u8),
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Esc,
    Enter,
    Tab,
    BackTab,
    Space,
}

// ── KeyCombo ─────────────────────────────────────────────────────────────────

/// A normalised key combination suitable for hashing and TOML serialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub code: KeyCodeRepr,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyCombo {
    /// Build from a crossterm `KeyEvent`, normalising modifiers and char case.
    pub fn from_key_event(event: &KeyEvent) -> Self {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let shift = event.modifiers.contains(KeyModifiers::SHIFT);
        let alt = event.modifiers.contains(KeyModifiers::ALT);

        let code = match event.code {
            KeyCode::Char(' ') => KeyCodeRepr::Space,
            KeyCode::Char(c) => KeyCodeRepr::Char(c.to_ascii_lowercase()),
            KeyCode::F(n) => KeyCodeRepr::F(n),
            KeyCode::Backspace => KeyCodeRepr::Backspace,
            KeyCode::Delete => KeyCodeRepr::Delete,
            KeyCode::Home => KeyCodeRepr::Home,
            KeyCode::End => KeyCodeRepr::End,
            KeyCode::PageUp => KeyCodeRepr::PageUp,
            KeyCode::PageDown => KeyCodeRepr::PageDown,
            KeyCode::Up => KeyCodeRepr::Up,
            KeyCode::Down => KeyCodeRepr::Down,
            KeyCode::Left => KeyCodeRepr::Left,
            KeyCode::Right => KeyCodeRepr::Right,
            KeyCode::Esc => KeyCodeRepr::Esc,
            KeyCode::Enter => KeyCodeRepr::Enter,
            KeyCode::Tab => KeyCodeRepr::Tab,
            KeyCode::BackTab => KeyCodeRepr::BackTab,
            // For anything else, use a null char placeholder (won't match anything).
            _ => KeyCodeRepr::Char('\0'),
        };

        Self {
            code,
            ctrl,
            shift,
            alt,
        }
    }
}

/// Canonical display: `ctrl+alt+shift+<key>`.
impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("ctrl+")?;
        }
        if self.alt {
            f.write_str("alt+")?;
        }
        if self.shift {
            f.write_str("shift+")?;
        }
        match &self.code {
            KeyCodeRepr::Char(c) => write!(f, "{c}"),
            KeyCodeRepr::F(n) => write!(f, "f{n}"),
            KeyCodeRepr::Backspace => f.write_str("backspace"),
            KeyCodeRepr::Delete => f.write_str("delete"),
            KeyCodeRepr::Home => f.write_str("home"),
            KeyCodeRepr::End => f.write_str("end"),
            KeyCodeRepr::PageUp => f.write_str("pageup"),
            KeyCodeRepr::PageDown => f.write_str("pagedown"),
            KeyCodeRepr::Up => f.write_str("up"),
            KeyCodeRepr::Down => f.write_str("down"),
            KeyCodeRepr::Left => f.write_str("left"),
            KeyCodeRepr::Right => f.write_str("right"),
            KeyCodeRepr::Esc => f.write_str("esc"),
            KeyCodeRepr::Enter => f.write_str("enter"),
            KeyCodeRepr::Tab => f.write_str("tab"),
            KeyCodeRepr::BackTab => f.write_str("backtab"),
            KeyCodeRepr::Space => f.write_str("space"),
        }
    }
}

/// Parse a key combo string like `"ctrl+shift+left"`.
///
/// Tokens are split on `+`, with modifiers (`ctrl`, `alt`, `shift`) consumed
/// first and the remainder interpreted as the key code.  Order of modifiers
/// does not matter.  All comparisons are case-insensitive.
impl FromStr for KeyCombo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty key combo".into());
        }

        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key_token: Option<&str> = None;

        for token in s.split('+') {
            let t = token.trim();
            match t.to_ascii_lowercase().as_str() {
                "ctrl" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                _ => {
                    if key_token.is_some() {
                        return Err(format!("multiple key tokens in '{s}'"));
                    }
                    key_token = Some(t);
                }
            }
        }

        let key_str = key_token.ok_or_else(|| format!("no key code in '{s}'"))?;
        let lower = key_str.to_ascii_lowercase();

        let code = if lower.len() == 1 {
            let c = lower.chars().next().unwrap();
            if c == ' ' {
                KeyCodeRepr::Space
            } else {
                KeyCodeRepr::Char(c)
            }
        } else if let Some(rest) = lower.strip_prefix('f') {
            if let Ok(n) = rest.parse::<u8>() {
                if (1..=12).contains(&n) {
                    KeyCodeRepr::F(n)
                } else {
                    return Err(format!("invalid function key: {key_str}"));
                }
            } else {
                parse_named_key(&lower)?
            }
        } else {
            parse_named_key(&lower)?
        };

        Ok(KeyCombo {
            code,
            ctrl,
            shift,
            alt,
        })
    }
}

fn parse_named_key(name: &str) -> Result<KeyCodeRepr, String> {
    match name {
        "backspace" => Ok(KeyCodeRepr::Backspace),
        "delete" | "del" => Ok(KeyCodeRepr::Delete),
        "home" => Ok(KeyCodeRepr::Home),
        "end" => Ok(KeyCodeRepr::End),
        "pageup" | "pgup" => Ok(KeyCodeRepr::PageUp),
        "pagedown" | "pgdn" | "pgdown" => Ok(KeyCodeRepr::PageDown),
        "up" => Ok(KeyCodeRepr::Up),
        "down" => Ok(KeyCodeRepr::Down),
        "left" => Ok(KeyCodeRepr::Left),
        "right" => Ok(KeyCodeRepr::Right),
        "esc" | "escape" => Ok(KeyCodeRepr::Esc),
        "enter" | "return" => Ok(KeyCodeRepr::Enter),
        "tab" => Ok(KeyCodeRepr::Tab),
        "backtab" => Ok(KeyCodeRepr::BackTab),
        "space" => Ok(KeyCodeRepr::Space),
        _ => Err(format!("unknown key: {name}")),
    }
}

// ── KeyBindings ──────────────────────────────────────────────────────────────

/// Runtime keybinding configuration: maps `KeyCombo` → `EditorAction`.
#[derive(Debug, Clone)]
pub struct KeyBindings {
    map: HashMap<KeyCombo, EditorAction>,
    /// action_name → key display string, for the help overlay.
    reverse: HashMap<String, String>,
}

impl KeyBindings {
    /// Load keybindings from `~/.config/txt/keybindings.toml`.
    ///
    /// If the file does not exist, writes the defaults and all preset files.
    /// On any error, silently falls back to defaults.
    pub fn load() -> Self {
        let result = Self::load_from_path(Self::keybindings_path().as_deref());
        // Ensure all preset files exist alongside the main keybindings file.
        Self::ensure_preset_files();
        result
    }

    /// Load from a specific path.  `None` → return defaults.
    pub fn load_from_path(path: Option<&Path>) -> Self {
        let defaults = Self::defaults();

        let path = match path {
            Some(p) => p,
            None => return defaults,
        };

        // If the file does not exist, write the defaults first.
        if !path.exists() {
            defaults.save_to(path);
            return defaults;
        }

        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return defaults,
        };

        let table: HashMap<String, String> = match toml::from_str(&text) {
            Ok(t) => t,
            Err(_) => return defaults,
        };

        Self::from_table(&table, &defaults)
    }

    /// Build keybindings from a parsed TOML table, merging with defaults.
    fn from_table(table: &HashMap<String, String>, defaults: &KeyBindings) -> Self {
        // Start with a full copy of the defaults (preserves multi-key actions).
        let mut map = defaults.map.clone();
        let mut reverse = defaults.reverse.clone();

        // Override with user bindings.
        for (action_name, key_str) in table {
            let action = match action_from_name(action_name) {
                Some(a) => a,
                None => continue, // unknown action, skip
            };

            let combo = match KeyCombo::from_str(key_str) {
                Ok(c) => c,
                Err(_) => continue, // unparseable key, skip
            };

            // Remove old combo for this action (if the user remapped it).
            if let Some(old_key_display) = reverse.get(action_name)
                && let Ok(old_combo) = KeyCombo::from_str(old_key_display)
            {
                map.remove(&old_combo);
            }

            // If this combo is currently bound to a different action, clear that
            // action's reverse entry before overwriting the forward mapping.
            if let Some(existing_action) = map.get(&combo)
                && existing_action != &action
                && let Some(existing_action_name) = action_to_name(existing_action)
            {
                reverse.remove(existing_action_name);
            }

            reverse.insert(action_name.clone(), combo.to_string());
            map.insert(combo, action);
        }

        KeyBindings { map, reverse }
    }

    /// Build the default keybindings matching the original hardcoded shortcuts.
    pub fn defaults() -> Self {
        let mut map = HashMap::new();
        let mut reverse = HashMap::new();

        let mut bind = |key_str: &str, action: EditorAction| {
            let combo = KeyCombo::from_str(key_str).expect("invalid default key combo");
            if let Some(name) = action_to_name(&action) {
                reverse.insert(name.to_string(), combo.to_string());
            }
            map.insert(combo, action);
        };

        // ── Function keys ──────────────────────────────────────────
        bind("f1", EditorAction::ToggleHelp);
        bind("f2", EditorAction::RenameSymbol);
        bind("shift+f3", EditorAction::SearchPrev);
        bind("f3", EditorAction::SearchNext);
        bind("shift+f12", EditorAction::FindReferences);
        bind("f12", EditorAction::GoToDefinition);
        bind("esc", EditorAction::CloseSearch);

        // ── Alt combinations ───────────────────────────────────────
        bind("alt+r", EditorAction::SearchToggleRegex);
        bind("alt+c", EditorAction::SearchToggleCaseSensitive);
        bind("alt+z", EditorAction::ToggleWordWrap);

        // ── Ctrl+Space / Ctrl+. ────────────────────────────────────
        bind("ctrl+space", EditorAction::TriggerCompletion);
        bind("ctrl+.", EditorAction::CodeAction);

        // ── Deletion ───────────────────────────────────────────────
        bind("ctrl+backspace", EditorAction::DeleteWordBackward);
        bind("backspace", EditorAction::DeleteBackward);
        bind("ctrl+delete", EditorAction::DeleteWordForward);
        bind("delete", EditorAction::DeleteForward);

        // ── Arrow keys ─────────────────────────────────────────────
        bind("alt+shift+up", EditorAction::SpawnCursorUp);
        bind("alt+shift+down", EditorAction::SpawnCursorDown);
        bind("alt+up", EditorAction::MoveLineUp);
        bind("alt+down", EditorAction::MoveLineDown);
        bind(
            "ctrl+shift+up",
            EditorAction::ExtendSelectionPage(super::action::Direction::Up),
        );
        bind(
            "ctrl+shift+down",
            EditorAction::ExtendSelectionPage(super::action::Direction::Down),
        );
        bind(
            "ctrl+shift+left",
            EditorAction::ExtendSelectionWord(super::action::Direction::Left),
        );
        bind(
            "ctrl+shift+right",
            EditorAction::ExtendSelectionWord(super::action::Direction::Right),
        );
        bind(
            "shift+up",
            EditorAction::ExtendSelection(super::action::Direction::Up),
        );
        bind(
            "shift+down",
            EditorAction::ExtendSelection(super::action::Direction::Down),
        );
        bind(
            "shift+left",
            EditorAction::ExtendSelection(super::action::Direction::Left),
        );
        bind(
            "shift+right",
            EditorAction::ExtendSelection(super::action::Direction::Right),
        );
        bind(
            "ctrl+up",
            EditorAction::Scroll(super::action::ScrollDir::Up),
        );
        bind(
            "ctrl+down",
            EditorAction::Scroll(super::action::ScrollDir::Down),
        );
        bind(
            "ctrl+left",
            EditorAction::MoveCursorWord(super::action::Direction::Left),
        );
        bind(
            "ctrl+right",
            EditorAction::MoveCursorWord(super::action::Direction::Right),
        );
        bind("up", EditorAction::MoveCursor(super::action::Direction::Up));
        bind(
            "down",
            EditorAction::MoveCursor(super::action::Direction::Down),
        );
        bind(
            "left",
            EditorAction::MoveCursor(super::action::Direction::Left),
        );
        bind(
            "right",
            EditorAction::MoveCursor(super::action::Direction::Right),
        );

        // ── Home / End ─────────────────────────────────────────────
        bind("ctrl+shift+home", EditorAction::ExtendSelectionFileStart);
        bind("ctrl+shift+end", EditorAction::ExtendSelectionFileEnd);
        bind("shift+home", EditorAction::ExtendSelectionHome);
        bind("shift+end", EditorAction::ExtendSelectionEnd);
        bind("ctrl+home", EditorAction::MoveCursorFileStart);
        bind("ctrl+end", EditorAction::MoveCursorFileEnd);
        bind("home", EditorAction::MoveCursorHome);
        bind("end", EditorAction::MoveCursorEnd);

        // ── Page Up / Down ─────────────────────────────────────────
        bind("ctrl+pageup", EditorAction::PrevTab);
        bind("ctrl+pagedown", EditorAction::NextTab);
        bind(
            "shift+pageup",
            EditorAction::ExtendSelectionPage(super::action::Direction::Up),
        );
        bind(
            "shift+pagedown",
            EditorAction::ExtendSelectionPage(super::action::Direction::Down),
        );
        bind(
            "pageup",
            EditorAction::MoveCursorPage(super::action::Direction::Up),
        );
        bind(
            "pagedown",
            EditorAction::MoveCursorPage(super::action::Direction::Down),
        );

        // ── Close tab ──────────────────────────────────────────────
        bind("ctrl+f4", EditorAction::CloseTab);

        // ── Ctrl+letter shortcuts ──────────────────────────────────
        bind("ctrl+q", EditorAction::Quit);
        bind("ctrl+z", EditorAction::Undo);
        bind("ctrl+y", EditorAction::Redo);
        bind("ctrl+d", EditorAction::DuplicateLine);
        bind("ctrl+a", EditorAction::SelectAll);
        bind("ctrl+w", EditorAction::AstExpandSelection);
        bind("ctrl+c", EditorAction::Copy);
        bind("ctrl+x", EditorAction::Cut);
        bind("ctrl+v", EditorAction::Paste(String::new()));
        bind("ctrl+s", EditorAction::SaveFile);
        bind("ctrl+n", EditorAction::NewFile);
        bind("ctrl+o", EditorAction::OpenFile);
        bind("ctrl+t", EditorAction::NewTab);
        bind("ctrl+g", EditorAction::JumpToLine);
        bind("ctrl+b", EditorAction::FocusSidebar);
        bind("ctrl+p", EditorAction::OpenFuzzyPicker);
        bind("ctrl+f", EditorAction::OpenSearch);
        bind("ctrl+h", EditorAction::OpenReplace);
        bind("ctrl+k", EditorAction::ShowHover);
        bind("ctrl+l", EditorAction::OpenLspConfig);
        bind("ctrl+r", EditorAction::OpenRecentFiles);
        bind("ctrl+/", EditorAction::ToggleLineComment);
        bind("ctrl+,", EditorAction::OpenSettings);
        bind("ctrl+[", EditorAction::PrevTab);
        bind("ctrl+]", EditorAction::NextTab);

        // ── Ctrl+Shift+letter shortcuts ────────────────────────────
        bind("ctrl+shift+z", EditorAction::Redo);
        bind("ctrl+shift+w", EditorAction::AstContractSelection);
        bind("ctrl+shift+s", EditorAction::SaveFileAs);
        bind("ctrl+shift+l", EditorAction::SelectAllOccurrences);
        bind("ctrl+shift+p", EditorAction::OpenCommandPalette);
        bind("ctrl+shift+e", EditorAction::OpenBufferSwitcher);
        bind("ctrl+shift+c", EditorAction::CopyFileReference);
        bind("ctrl+shift+b", EditorAction::ToggleSidebar);
        bind("ctrl+shift+n", EditorAction::SidebarNewFolder);

        KeyBindings { map, reverse }
    }

    /// Look up an action for the given key combo.
    pub fn lookup(&self, combo: &KeyCombo) -> Option<&EditorAction> {
        self.map.get(combo)
    }

    /// Get the display string for a key bound to the named action.
    pub fn display_key_for_action(&self, action_name: &str) -> Option<&str> {
        self.reverse.get(action_name).map(|s| s.as_str())
    }

    /// Path to the keybindings config file.
    pub fn keybindings_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("txt").join("keybindings.toml"))
    }

    /// Write bindings to a TOML file.  Silently ignores errors.
    fn save_to(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Build a sorted list for deterministic output.
        let mut entries: Vec<(&str, &str)> = self
            .reverse
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        entries.sort_by_key(|(name, _)| *name);

        let mut out = String::from(
            "# Keyboard shortcuts for txt editor.\n\
             # Format: action_name = \"key+combo\"\n\
             # Modifiers: ctrl, alt, shift (order doesn't matter)\n\
             # Keys: a-z, 0-9, f1-f12, up, down, left, right, home, end,\n\
             #       pageup, pagedown, backspace, delete, esc, enter, tab, space\n\
             # Examples: \"ctrl+s\", \"ctrl+shift+p\", \"alt+z\", \"f1\"\n\n",
        );

        for (name, key) in &entries {
            out.push_str(&format!("{name} = \"{key}\"\n"));
        }

        let _ = std::fs::write(path, out);
    }

    /// Total number of entries in the help overlay.
    #[allow(dead_code)]
    pub fn entry_count(&self) -> usize {
        self.reverse.len()
    }

    // ── Preset support ──────────────────────────────────────────────────────

    /// Path to the preset file for the given preset.
    pub fn preset_path(preset: &KeymapPreset) -> Option<PathBuf> {
        let filename = match preset {
            KeymapPreset::Default => "keybindings-default.toml",
            KeymapPreset::IntellijIdea => "keybindings-intellij.toml",
            KeymapPreset::VsCode => "keybindings-vscode.toml",
        };
        dirs::home_dir().map(|h| h.join(".config").join("txt").join(filename))
    }

    /// Write all preset files if they don't already exist.
    fn ensure_preset_files() {
        for preset in KeymapPreset::ALL {
            if let Some(path) = Self::preset_path(preset)
                && !path.exists()
            {
                let bindings = Self::for_preset(preset);
                bindings.save_to(&path);
            }
        }
    }

    /// Get the keybindings for a given preset.
    pub fn for_preset(preset: &KeymapPreset) -> Self {
        match preset {
            KeymapPreset::Default => Self::defaults(),
            KeymapPreset::IntellijIdea => Self::intellij_defaults(),
            KeymapPreset::VsCode => Self::vscode_defaults(),
        }
    }

    /// Copy the selected preset file to `keybindings.toml`.
    pub fn apply_preset(preset: &KeymapPreset) {
        let Some(main_path) = Self::keybindings_path() else {
            return;
        };
        let Some(preset_path) = Self::preset_path(preset) else {
            return;
        };
        // If the preset file doesn't exist, generate it first.
        if !preset_path.exists() {
            let bindings = Self::for_preset(preset);
            bindings.save_to(&preset_path);
        }
        let _ = std::fs::copy(&preset_path, &main_path);
    }

    /// Build IntelliJ IDEA macOS-style keybindings (Cmd→Ctrl, Opt→Alt for terminal).
    ///
    /// Based on the IntelliJ IDEA macOS default keymap, translating Cmd→Ctrl and
    /// Opt→Alt for terminal use. Only actions that exist in this editor are mapped.
    fn intellij_defaults() -> Self {
        let mut kb = Self::defaults();

        // ── Word navigation: Opt+Arrow (not Ctrl+Arrow) ────────────────
        kb.rebind("move_cursor_word_left", "alt+left");
        kb.rebind("move_cursor_word_right", "alt+right");
        kb.rebind("extend_selection_word_left", "alt+shift+left");
        kb.rebind("extend_selection_word_right", "alt+shift+right");

        // ── Word deletion: Opt+Backspace/Delete ────────────────────────
        kb.rebind("delete_word_backward", "alt+backspace");
        kb.rebind("delete_word_forward", "alt+delete");

        // ── Recent Files: Cmd+E → ctrl+e ────────────────────────────────
        // (must come before open_replace to free ctrl+r)
        kb.rebind("open_recent_files", "ctrl+e");

        // ── Find & Replace: Cmd+R → ctrl+r ─────────────────────────────
        kb.rebind("open_replace", "ctrl+r");

        // ── Find Next/Prev: Cmd+G / Cmd+Shift+G ────────────────────────
        // (jump_to_line moves to ctrl+g is default, but IntelliJ uses Cmd+L
        //  for jump-to-line. We keep ctrl+g for jump_to_line and use f3/shift+f3
        //  which are also already bound for search_next/search_prev.)

        // ── Find Action: Cmd+Shift+A → ctrl+shift+a ────────────────────
        kb.rebind("open_command_palette", "ctrl+shift+a");

        // ── Navigate File: Cmd+Shift+O → ctrl+shift+o ──────────────────
        kb.rebind("open_fuzzy_picker", "ctrl+shift+o");

        // ── Tab navigation: Cmd+Shift+[ / ] → ctrl+alt+left / right ────
        kb.rebind("prev_tab", "ctrl+alt+left");
        kb.rebind("next_tab", "ctrl+alt+right");

        // ── Project Tool Window: Alt+1 (sidebar) ─────────────────────────
        // (must come before go_to_definition to free ctrl+b)
        kb.rebind("focus_sidebar", "alt+1");

        // ── Go to Declaration: Cmd+B → ctrl+b ──────────────────────────
        kb.rebind("go_to_definition", "ctrl+b");

        // ── Find Usages: Alt+F7 ────────────────────────────────────────
        kb.rebind("find_references", "alt+f7");

        // ── Rename: Shift+F6 ───────────────────────────────────────────
        kb.rebind("rename_symbol", "shift+f6");

        // ── Show Intention Actions / Quick Fix: Alt+Enter ───────────────
        kb.rebind("code_action", "alt+enter");

        // ── Quick Documentation: Ctrl+Q → ctrl+j ───────────────────────
        // (ctrl+q is quit; IntelliJ uses F1 for docs too, but F1 is help)
        kb.rebind("show_hover", "ctrl+j");

        // ── Duplicate Line: Cmd+D → ctrl+d (same as default) ───────────
        // (already the default)

        // ── Extend/Shrink Selection: Cmd+W / Cmd+Shift+W ───────────────
        // (already the default: ctrl+w / ctrl+shift+w)

        kb
    }

    /// Build VS Code macOS-style keybindings (Cmd→Ctrl for terminal).
    ///
    /// Based on the VS Code macOS default keymap, translating Cmd→Ctrl and
    /// Opt→Alt for terminal use. The editor's defaults are already very close
    /// to VS Code, so this preset has fewer changes than IntelliJ.
    fn vscode_defaults() -> Self {
        let mut kb = Self::defaults();

        // ── Find & Replace: Cmd+Opt+F → ctrl+alt+f ─────────────────────
        kb.rebind("open_replace", "ctrl+alt+f");

        // ── Duplicate Line: Opt+Shift+Down in VS Code, but that conflicts
        //    with spawn_cursor_down, so use Ctrl+Shift+D ─────────────────
        kb.rebind("duplicate_line", "ctrl+shift+d");

        // ── Word navigation: Opt+Arrow (VS Code macOS uses Opt for word nav)
        kb.rebind("move_cursor_word_left", "alt+left");
        kb.rebind("move_cursor_word_right", "alt+right");
        kb.rebind("extend_selection_word_left", "alt+shift+left");
        kb.rebind("extend_selection_word_right", "alt+shift+right");

        // ── Word deletion: Opt+Backspace/Delete ────────────────────────
        kb.rebind("delete_word_backward", "alt+backspace");
        kb.rebind("delete_word_forward", "alt+delete");

        // ── Go to Symbol in File: Cmd+Shift+O → ctrl+shift+o ───────────
        kb.rebind("open_fuzzy_picker", "ctrl+shift+o");

        // ── Multi-cursor: Cmd+Alt+Up/Down → ctrl+alt+up/down ───────────
        kb.rebind("spawn_cursor_up", "ctrl+alt+up");
        kb.rebind("spawn_cursor_down", "ctrl+alt+down");

        // ── Smart Select: Ctrl+Shift+Right/Left ────────────────────────
        // (ctrl+shift+left/right freed by word selection moving to alt+shift)
        kb.rebind("ast_expand_selection", "ctrl+shift+right");
        kb.rebind("ast_contract_selection", "ctrl+shift+left");

        kb
    }

    /// Rebind an action to a new key combo, removing the old binding.
    fn rebind(&mut self, action_name: &str, new_key: &str) {
        let action = match action_from_name(action_name) {
            Some(a) => a,
            None => return,
        };
        let new_combo = match KeyCombo::from_str(new_key) {
            Ok(c) => c,
            Err(_) => return,
        };

        // Remove old combo for this action.
        if let Some(old_display) = self.reverse.get(action_name)
            && let Ok(old_combo) = KeyCombo::from_str(old_display)
        {
            self.map.remove(&old_combo);
        }

        // If the new combo is currently bound to a different action, clear that
        // action's reverse entry before overwriting the forward mapping.
        if let Some(existing_action) = self.map.get(&new_combo)
            && existing_action != &action
            && let Some(existing_action_name) = action_to_name(existing_action)
        {
            self.reverse.remove(existing_action_name);
        }

        self.reverse
            .insert(action_name.to_string(), new_combo.to_string());
        self.map.insert(new_combo, action);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_key() {
        let combo: KeyCombo = "ctrl+s".parse().unwrap();
        assert!(combo.ctrl);
        assert!(!combo.shift);
        assert!(!combo.alt);
        assert_eq!(combo.code, KeyCodeRepr::Char('s'));
    }

    #[test]
    fn parse_ctrl_shift() {
        let combo: KeyCombo = "ctrl+shift+p".parse().unwrap();
        assert!(combo.ctrl);
        assert!(combo.shift);
        assert!(!combo.alt);
        assert_eq!(combo.code, KeyCodeRepr::Char('p'));
    }

    #[test]
    fn parse_function_key() {
        let combo: KeyCombo = "f1".parse().unwrap();
        assert_eq!(combo.code, KeyCodeRepr::F(1));
        assert!(!combo.ctrl);
        assert!(!combo.shift);
    }

    #[test]
    fn parse_shift_f3() {
        let combo: KeyCombo = "shift+f3".parse().unwrap();
        assert!(combo.shift);
        assert_eq!(combo.code, KeyCodeRepr::F(3));
    }

    #[test]
    fn parse_alt_key() {
        let combo: KeyCombo = "alt+z".parse().unwrap();
        assert!(combo.alt);
        assert_eq!(combo.code, KeyCodeRepr::Char('z'));
    }

    #[test]
    fn parse_named_keys() {
        assert_eq!(
            "backspace".parse::<KeyCombo>().unwrap().code,
            KeyCodeRepr::Backspace
        );
        assert_eq!(
            "pageup".parse::<KeyCombo>().unwrap().code,
            KeyCodeRepr::PageUp
        );
        assert_eq!("esc".parse::<KeyCombo>().unwrap().code, KeyCodeRepr::Esc);
        assert_eq!(
            "space".parse::<KeyCombo>().unwrap().code,
            KeyCodeRepr::Space
        );
    }

    #[test]
    fn display_roundtrips() {
        let combos = &["ctrl+s", "ctrl+shift+p", "f1", "alt+z", "ctrl+backspace"];
        for &input in combos {
            let combo: KeyCombo = input.parse().unwrap();
            let displayed = combo.to_string();
            let reparsed: KeyCombo = displayed.parse().unwrap();
            assert_eq!(combo, reparsed, "round-trip failed for '{input}'");
        }
    }

    #[test]
    fn parse_error_on_empty() {
        assert!("".parse::<KeyCombo>().is_err());
    }

    #[test]
    fn parse_error_on_unknown_key() {
        assert!("ctrl+foobar".parse::<KeyCombo>().is_err());
    }

    #[test]
    fn defaults_have_all_expected_actions() {
        let kb = KeyBindings::defaults();
        // Spot-check a few
        assert!(kb.display_key_for_action("quit").is_some());
        assert!(kb.display_key_for_action("save_file").is_some());
        assert!(kb.display_key_for_action("undo").is_some());
        assert!(kb.display_key_for_action("toggle_help").is_some());
        assert!(kb.display_key_for_action("paste").is_some());
    }

    #[test]
    fn defaults_lookup_ctrl_q() {
        let kb = KeyBindings::defaults();
        let combo: KeyCombo = "ctrl+q".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::Quit));
    }

    #[test]
    fn load_from_none_returns_defaults() {
        let kb = KeyBindings::load_from_path(None);
        let defaults = KeyBindings::defaults();
        assert_eq!(kb.map.len(), defaults.map.len());
    }

    #[test]
    fn from_table_overrides_single_binding() {
        let defaults = KeyBindings::defaults();
        let mut table = HashMap::new();
        table.insert("quit".to_string(), "ctrl+shift+q".to_string());
        let kb = KeyBindings::from_table(&table, &defaults);

        // New binding works
        let new_combo: KeyCombo = "ctrl+shift+q".parse().unwrap();
        assert_eq!(kb.lookup(&new_combo), Some(&EditorAction::Quit));

        // Old binding removed
        let old_combo: KeyCombo = "ctrl+q".parse().unwrap();
        assert_eq!(kb.lookup(&old_combo), None);
    }

    #[test]
    fn from_table_ignores_unknown_actions() {
        let defaults = KeyBindings::defaults();
        let mut table = HashMap::new();
        table.insert("nonexistent_action".to_string(), "ctrl+z".to_string());
        let kb = KeyBindings::from_table(&table, &defaults);
        // Should still have defaults
        assert_eq!(kb.map.len(), defaults.map.len());
    }

    #[test]
    fn from_key_event_normalizes() {
        let event = KeyEvent::new(
            KeyCode::Char('S'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let combo = KeyCombo::from_key_event(&event);
        assert!(combo.ctrl);
        assert!(combo.shift);
        assert_eq!(combo.code, KeyCodeRepr::Char('s'));
    }

    #[test]
    fn action_name_roundtrip() {
        let actions = [
            EditorAction::Quit,
            EditorAction::Undo,
            EditorAction::MoveCursor(super::super::action::Direction::Up),
            EditorAction::Paste(String::new()),
            EditorAction::Scroll(super::super::action::ScrollDir::Down),
        ];
        for action in &actions {
            let name = action_to_name(action).unwrap();
            let roundtripped = action_from_name(name).unwrap();
            assert_eq!(*action, roundtripped, "roundtrip failed for {name}");
        }
    }

    #[test]
    fn intellij_defaults_word_nav_uses_alt() {
        let kb = KeyBindings::intellij_defaults();
        let alt_left: KeyCombo = "alt+left".parse().unwrap();
        assert_eq!(
            kb.lookup(&alt_left),
            Some(&EditorAction::MoveCursorWord(
                super::super::action::Direction::Left
            ))
        );
        // ctrl+left should NOT be bound to word nav in IntelliJ preset
        let ctrl_left: KeyCombo = "ctrl+left".parse().unwrap();
        assert_ne!(
            kb.lookup(&ctrl_left),
            Some(&EditorAction::MoveCursorWord(
                super::super::action::Direction::Left
            ))
        );
    }

    #[test]
    fn intellij_defaults_replace_is_ctrl_r() {
        let kb = KeyBindings::intellij_defaults();
        let combo: KeyCombo = "ctrl+r".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::OpenReplace));
    }

    #[test]
    fn intellij_defaults_go_to_definition_is_ctrl_b() {
        let kb = KeyBindings::intellij_defaults();
        let combo: KeyCombo = "ctrl+b".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::GoToDefinition));
    }

    #[test]
    fn intellij_defaults_find_references_is_alt_f7() {
        let kb = KeyBindings::intellij_defaults();
        let combo: KeyCombo = "alt+f7".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::FindReferences));
    }

    #[test]
    fn intellij_defaults_rename_is_shift_f6() {
        let kb = KeyBindings::intellij_defaults();
        let combo: KeyCombo = "shift+f6".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::RenameSymbol));
    }

    #[test]
    fn intellij_defaults_sidebar_is_alt_1() {
        let kb = KeyBindings::intellij_defaults();
        let combo: KeyCombo = "alt+1".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::FocusSidebar));
    }

    #[test]
    fn vscode_defaults_replace_is_ctrl_alt_f() {
        let kb = KeyBindings::vscode_defaults();
        let combo: KeyCombo = "ctrl+alt+f".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::OpenReplace));
    }

    #[test]
    fn vscode_defaults_duplicate_is_ctrl_shift_d() {
        let kb = KeyBindings::vscode_defaults();
        let combo: KeyCombo = "ctrl+shift+d".parse().unwrap();
        assert_eq!(kb.lookup(&combo), Some(&EditorAction::DuplicateLine));
    }

    #[test]
    fn vscode_defaults_word_nav_uses_alt() {
        let kb = KeyBindings::vscode_defaults();
        let alt_left: KeyCombo = "alt+left".parse().unwrap();
        assert_eq!(
            kb.lookup(&alt_left),
            Some(&EditorAction::MoveCursorWord(
                super::super::action::Direction::Left
            ))
        );
        let alt_right: KeyCombo = "alt+right".parse().unwrap();
        assert_eq!(
            kb.lookup(&alt_right),
            Some(&EditorAction::MoveCursorWord(
                super::super::action::Direction::Right
            ))
        );
    }

    #[test]
    fn vscode_defaults_multi_cursor_uses_ctrl_alt() {
        let kb = KeyBindings::vscode_defaults();
        let combo_up: KeyCombo = "ctrl+alt+up".parse().unwrap();
        assert_eq!(kb.lookup(&combo_up), Some(&EditorAction::SpawnCursorUp));
        let combo_down: KeyCombo = "ctrl+alt+down".parse().unwrap();
        assert_eq!(kb.lookup(&combo_down), Some(&EditorAction::SpawnCursorDown));
    }

    #[test]
    fn vscode_defaults_ast_selection_uses_ctrl_shift_arrow() {
        let kb = KeyBindings::vscode_defaults();
        let combo_right: KeyCombo = "ctrl+shift+right".parse().unwrap();
        assert_eq!(
            kb.lookup(&combo_right),
            Some(&EditorAction::AstExpandSelection)
        );
        let combo_left: KeyCombo = "ctrl+shift+left".parse().unwrap();
        assert_eq!(
            kb.lookup(&combo_left),
            Some(&EditorAction::AstContractSelection)
        );
    }

    #[test]
    fn preset_path_returns_distinct_files() {
        let p1 = KeyBindings::preset_path(&KeymapPreset::Default);
        let p2 = KeyBindings::preset_path(&KeymapPreset::IntellijIdea);
        let p3 = KeyBindings::preset_path(&KeymapPreset::VsCode);
        assert_ne!(p1, p2);
        assert_ne!(p2, p3);
        assert_ne!(p1, p3);
    }

    #[test]
    fn for_preset_returns_correct_type() {
        let default = KeyBindings::for_preset(&KeymapPreset::Default);
        let intellij = KeyBindings::for_preset(&KeymapPreset::IntellijIdea);
        // IntelliJ should differ from default (word nav)
        let alt_left: KeyCombo = "alt+left".parse().unwrap();
        assert!(intellij.lookup(&alt_left).is_some());
        assert!(default.lookup(&alt_left).is_none());
    }
}
