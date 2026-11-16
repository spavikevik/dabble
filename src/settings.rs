//! User settings, stored as `key = value` lines in
//! `~/Library/Application Support/Dabble/settings.txt`.

use std::path::PathBuf;

use crate::theme::ThemePref;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interface {
    /// The Paint-style ribbon drawn by Dabble.
    Classic,
    /// Native macOS toolbar and a Liquid Glass inspector.
    Native,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub interface: Interface,
    pub theme: ThemePref,
}

impl Default for Settings {
    fn default() -> Self {
        Self { interface: Interface::Classic, theme: ThemePref::System }
    }
}

fn path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Dabble/settings.txt"))
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match (k.trim(), v.trim()) {
                ("interface", "native") => s.interface = Interface::Native,
                ("interface", "classic") => s.interface = Interface::Classic,
                ("theme", "system") => s.theme = ThemePref::System,
                ("theme", "light") => s.theme = ThemePref::Light,
                ("theme", "dark") => s.theme = ThemePref::Dark,
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let interface = match self.interface {
            Interface::Classic => "classic",
            Interface::Native => "native",
        };
        let theme = match self.theme {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        };
        format!("interface = {interface}\ntheme = {theme}\n")
    }

    pub fn load() -> Self {
        let mut s: Self =
            path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Self::parse(&t)).unwrap_or_default();
        // For trying an interface without changing the saved setting.
        match std::env::var("DABBLE_INTERFACE").as_deref() {
            Ok("native") => s.interface = Interface::Native,
            Ok("classic") => s.interface = Interface::Classic,
            _ => {}
        }
        match std::env::var("DABBLE_THEME").as_deref() {
            Ok("light") => s.theme = ThemePref::Light,
            Ok("dark") => s.theme = ThemePref::Dark,
            _ => {}
        }
        s
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, self.to_text());
    }
}

/// Whether this Mac can show the native interface.
pub fn native_available() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings { interface: Interface::Native, theme: ThemePref::Dark };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("junk\ninterface = native"), Settings { interface: Interface::Native, ..Default::default() });
    }
}
