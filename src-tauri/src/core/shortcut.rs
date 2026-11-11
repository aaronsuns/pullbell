//! Parsing and validating global shortcuts.
//!
//! Canonical form is a Tauri accelerator string with modifiers in macOS order:
//! `Ctrl+Alt+Shift+Cmd+G`. Display form uses the glyphs: `⌃⌥⇧⌘G`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Upper-case key name: `G`, `5`, `SPACE`, `TAB`, `F5`, `UP`, …
    pub key: String,
}

const NAMED_KEYS: &[&str] = &[
    "SPACE",
    "TAB",
    "ENTER",
    "ESCAPE",
    "BACKSPACE",
    "DELETE",
    "UP",
    "DOWN",
    "LEFT",
    "RIGHT",
    "HOME",
    "END",
    "PAGEUP",
    "PAGEDOWN",
    "COMMA",
    "PERIOD",
    "SLASH",
    "SEMICOLON",
    "QUOTE",
    "BACKQUOTE",
    "MINUS",
    "EQUAL",
    "BRACKETLEFT",
    "BRACKETRIGHT",
    "BACKSLASH",
];

fn canonical_key(raw: &str) -> Option<String> {
    let k = raw.trim().to_ascii_uppercase();
    let k = k
        .strip_prefix("KEY")
        .filter(|r| r.len() == 1)
        .or_else(|| k.strip_prefix("DIGIT"))
        .or_else(|| k.strip_prefix("ARROW"))
        .map(str::to_string)
        .unwrap_or(k);
    let k = match k.as_str() {
        "ESC" => "ESCAPE".to_string(),
        "RETURN" => "ENTER".to_string(),
        "`" => "BACKQUOTE".to_string(),
        "," => "COMMA".to_string(),
        "." => "PERIOD".to_string(),
        "/" => "SLASH".to_string(),
        _ => k,
    };
    let single = k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric());
    let fkey = k
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=20).contains(&n));
    (single || fkey || NAMED_KEYS.contains(&k.as_str())).then_some(k)
}

impl Shortcut {
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut sc = Shortcut {
            ctrl: false,
            alt: false,
            shift: false,
            cmd: false,
            key: String::new(),
        };
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last().ok_or("empty shortcut")?;
        for m in mods {
            match m.to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => sc.ctrl = true,
                "ALT" | "OPTION" => sc.alt = true,
                "SHIFT" => sc.shift = true,
                "CMD" | "COMMAND" | "SUPER" | "META" | "COMMANDORCONTROL" | "CMDORCTRL" => {
                    sc.cmd = true
                }
                other => return Err(format!("unknown modifier {other}")),
            }
        }
        sc.key = canonical_key(key).ok_or_else(|| format!("unsupported key {key}"))?;
        Ok(sc)
    }

    pub fn accelerator(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.cmd, "Cmd"),
        ] {
            if on {
                parts.push(name.to_string());
            }
        }
        parts.push(self.key.clone());
        parts.join("+")
    }

    pub fn display(&self) -> String {
        let mut out = String::new();
        for (on, g) in [
            (self.ctrl, "⌃"),
            (self.alt, "⌥"),
            (self.shift, "⇧"),
            (self.cmd, "⌘"),
        ] {
            if on {
                out.push_str(g);
            }
        }
        let key = match self.key.as_str() {
            "SPACE" => "Space",
            "TAB" => "⇥",
            "ENTER" => "↩",
            "ESCAPE" => "⎋",
            "UP" => "↑",
            "DOWN" => "↓",
            "LEFT" => "←",
            "RIGHT" => "→",
            "BACKQUOTE" => "`",
            "COMMA" => ",",
            "PERIOD" => ".",
            "SLASH" => "/",
            k => k,
        };
        out.push_str(key);
        out
    }

    fn mods(&self) -> (bool, bool, bool, bool) {
        (self.ctrl, self.alt, self.shift, self.cmd)
    }
}

/// Combinations macOS (or every app) owns. Written as accelerators.
const RESERVED: &[&str] = &[
    "Cmd+SPACE",
    "Ctrl+SPACE",
    "Alt+Cmd+SPACE",
    "Ctrl+Cmd+SPACE",
    "Cmd+TAB",
    "Shift+Cmd+TAB",
    "Cmd+BACKQUOTE",
    "Shift+Cmd+3",
    "Shift+Cmd+4",
    "Shift+Cmd+5",
    "Ctrl+Shift+Cmd+3",
    "Ctrl+Shift+Cmd+4",
    "Alt+Cmd+ESCAPE",
    "Ctrl+Cmd+Q",
    "Ctrl+Cmd+F",
    "Alt+Cmd+D",
    "Shift+Cmd+Q",
    "Alt+Shift+Cmd+Q",
    "Ctrl+UP",
    "Ctrl+DOWN",
    "Ctrl+LEFT",
    "Ctrl+RIGHT",
];

/// Validate a recorded shortcut. `others` are the other Pullbell shortcuts (canonical form).
/// Returns the canonical accelerator or a short human reason.
pub fn validate(candidate: &str, others: &[&str]) -> Result<String, String> {
    let sc = Shortcut::parse(candidate)?;
    let (ctrl, alt, _shift, cmd) = sc.mods();
    if !(ctrl || alt || cmd) {
        return Err("Add at least one of ⌃ ⌥ ⌘".into());
    }
    let acc = sc.accelerator();
    if RESERVED.contains(&acc.as_str()) {
        return Err(format!("{} is reserved by macOS", sc.display()));
    }
    if sc.mods() == (false, false, false, true) && sc.key.len() == 1 {
        return Err(format!("{} is used by every app", sc.display()));
    }
    for o in others {
        if let Ok(other) = Shortcut::parse(o) {
            if other == sc {
                return Err(format!(
                    "{} is already used by another shortcut",
                    sc.display()
                ));
            }
        }
    }
    Ok(acc)
}

/// Display form for a stored accelerator; empty when unset or unparsable.
pub fn display(acc: Option<&str>) -> String {
    acc.and_then(|a| Shortcut::parse(a).ok())
        .map(|s| s.display())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_canonicalizes() {
        let s = Shortcut::parse("command+control+KeyG").unwrap();
        assert_eq!(s.accelerator(), "Ctrl+Cmd+G");
        assert_eq!(s.display(), "⌃⌘G");
        assert_eq!(
            Shortcut::parse("Shift+Ctrl+Cmd+g").unwrap().display(),
            "⌃⇧⌘G"
        );
        assert_eq!(
            Shortcut::parse("Alt+Digit5").unwrap().accelerator(),
            "Alt+5"
        );
        assert_eq!(Shortcut::parse("Ctrl+ArrowUp").unwrap().key, "UP");
        assert!(Shortcut::parse("Hyper+G").is_err());
        assert!(Shortcut::parse("Ctrl+Fn").is_err());
    }

    #[test]
    fn needs_a_real_modifier() {
        assert!(validate("G", &[]).unwrap_err().contains("⌃ ⌥ ⌘"));
        assert!(validate("Shift+G", &[]).is_err());
        assert_eq!(validate("Ctrl+Cmd+G", &[]).unwrap(), "Ctrl+Cmd+G");
        assert_eq!(validate("Alt+F5", &[]).unwrap(), "Alt+F5");
    }

    #[test]
    fn rejects_macos_reserved() {
        for s in [
            "Cmd+Space",
            "Cmd+Tab",
            "Shift+Cmd+3",
            "Shift+Cmd+4",
            "Shift+Cmd+5",
            "Ctrl+Space",
        ] {
            let err = validate(s, &[]).unwrap_err();
            assert!(err.contains("reserved"), "{s}: {err}");
        }
        assert!(validate("Cmd+Q", &[]).unwrap_err().contains("every app"));
        assert!(validate("Cmd+W", &[]).is_err());
    }

    #[test]
    fn rejects_duplicates_regardless_of_spelling() {
        let err = validate("Control+Command+G", &["Ctrl+Cmd+G"]).unwrap_err();
        assert!(err.contains("already used"));
        assert!(validate("Ctrl+Cmd+P", &["Ctrl+Cmd+G"]).is_ok());
    }

    #[test]
    fn display_helper() {
        assert_eq!(display(Some("Ctrl+Shift+Cmd+G")), "⌃⇧⌘G");
        assert_eq!(display(None), "");
        assert_eq!(display(Some("garbage+")), "");
    }
}
