//! The language the game's own screens are shown in.
//!
//! Every English text a player sees goes through [`t`] (or [`tf`] when it
//! has numbers or names in it), which looks it up in the translations
//! below, keyed by the exact English text. Anything without a translation
//! stays in English.
//!
//! The translations are plain text files (`lang/pt-BR.txt`,
//! `lang/es-ES.txt`), built into the game: one `English = Translation`
//! per line. The language is a global, so any code can use [`t`] without
//! asking Bevy for anything; Settings sets it (see
//! [`crate::settings::ClientSettings::language`]).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    PortugueseBrazil,
    SpanishSpain,
}

impl Language {
    pub const ALL: [Language; 3] = [
        Language::English,
        Language::PortugueseBrazil,
        Language::SpanishSpain,
    ];

    /// How it's written in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::PortugueseBrazil => "pt-BR",
            Language::SpanishSpain => "es-ES",
        }
    }

    /// A language from a code like `pt-BR`, `es_ES.UTF-8` or `en-GB`
    /// (only the first two letters count).
    pub fn from_code(code: &str) -> Option<Self> {
        match code.get(..2)?.to_ascii_lowercase().as_str() {
            "en" => Some(Language::English),
            "pt" => Some(Language::PortugueseBrazil),
            "es" => Some(Language::SpanishSpain),
            _ => None,
        }
    }

    /// Its name, in itself (for the Settings button).
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::PortugueseBrazil => "Português",
            Language::SpanishSpain => "Español",
        }
    }

    /// The next one, for the Settings button.
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|l| *l == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// The computer's language, if we have it (otherwise English).
    pub fn from_system() -> Self {
        sys_locale::get_locale()
            .or_else(|| {
                ["LC_ALL", "LC_MESSAGES", "LANG"]
                    .iter()
                    .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            })
            .and_then(|l| Self::from_code(&l))
            .unwrap_or_default()
    }

    fn translations(self) -> Option<&'static HashMap<String, &'static str>> {
        static PT: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
        static ES: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
        match self {
            Language::English => None,
            Language::PortugueseBrazil => Some(PT.get_or_init(|| parse_leaked(PT_BR))),
            Language::SpanishSpain => Some(ES.get_or_init(|| parse_leaked(ES_ES))),
        }
    }
}

const PT_BR: &str = include_str!("../lang/pt-BR.txt");
const ES_ES: &str = include_str!("../lang/es-ES.txt");

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language the screens are shown in now.
pub fn current() -> Language {
    Language::ALL
        .get(CURRENT.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}

/// Show the screens in `language` from now on.
pub fn set(language: Language) {
    let i = Language::ALL
        .iter()
        .position(|l| *l == language)
        .unwrap_or(0);
    CURRENT.store(i as u8, Ordering::Relaxed);
}

/// True when the language has changed since `seen` was last updated (and
/// updates it): for text that's made once and kept, to redo it.
pub fn changed(seen: &mut Option<Language>) -> bool {
    let now = Some(current());
    let changed = *seen != now;
    *seen = now;
    changed
}

/// The translation of an English text, or the English if there isn't one.
pub fn t(english: &'static str) -> &'static str {
    lookup(english).unwrap_or(english)
}

/// The translation of any English text, if there is one.
pub fn lookup(english: &str) -> Option<&'static str> {
    current().translations()?.get(english).copied()
}

/// [`t`] for a text with blanks (`{}`) in it, filled with `args` in order:
/// `tf("Mods on: {}", &[&n.to_string()])`.
pub fn tf(english: &'static str, args: &[&str]) -> String {
    fill(t(english), args)
}

fn fill(text: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut args = args.iter();
    let mut rest = text;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(args.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// Read a translation file: `English = Translation` lines; `#` starts a
/// comment line; `\n` is a line break and `\\` a backslash. The first
/// ` = ` splits the line (so the English mustn't contain ` = `).
fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, v)| (unescape(k.trim_end()), unescape(v.trim_start())))
        .collect()
}

fn parse_leaked(text: &str) -> HashMap<String, &'static str> {
    parse(text)
        .into_iter()
        .map(|(k, v)| (k, &*Box::leak(v.into_boxed_str())))
        .collect()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every `.rs` file in the client's `src`, read from disk.
    fn sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    out.push((path.display().to_string(), text));
                }
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        out
    }

    /// Read the Rust string literal that starts at `s` (just after its
    /// opening quote).
    fn read_literal(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => return out,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    // A line continuation: skip the break and the indent.
                    Some('\n') => {
                        while chars.peek().is_some_and(|c| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    Some(other) => out.push(other),
                    None => {}
                },
                _ => out.push(c),
            }
        }
        panic!("unfinished string literal");
    }

    /// Every literal passed to `t("…")` or `tf("…", …)` in the client
    /// (but not this file's examples).
    fn used_keys() -> Vec<(String, String)> {
        let mut keys = Vec::new();
        for (file, text) in sources() {
            if file.ends_with("lang.rs") {
                continue;
            }
            for call in ["t(", "tf("] {
                for (i, _) in text.match_indices(call) {
                    let before = text[..i].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = text[i + call.len()..].trim_start();
                    if let Some(literal) = rest.strip_prefix('"') {
                        keys.push((file.clone(), read_literal(literal)));
                    }
                }
            }
        }
        keys
    }

    /// Texts translated by passing them through `t` from a list rather
    /// than as a literal.
    fn listed_keys() -> Vec<String> {
        use crate::render::skin_textures::{BUILTIN_SKINS, MAKER_PARTS};
        let mut keys: Vec<String> = Vec::new();
        for (part, options) in MAKER_PARTS {
            keys.push(part.to_owned());
            keys.extend(options.iter().map(|o| o.to_string()));
        }
        keys.extend(BUILTIN_SKINS.iter().map(|(_, l)| l.to_string()));
        keys.extend(crate::menu::GAME_MODES.iter().map(|(_, l)| l.to_string()));
        keys
    }

    /// "Part: choice" keys for the skin maker, only there when a choice
    /// needs saying differently for that part (see `crate::menu`).
    fn maker_keys() -> Vec<String> {
        crate::render::skin_textures::MAKER_PARTS
            .iter()
            .flat_map(|(p, os)| os.iter().map(move |o| format!("{p}: {o}")))
            .collect()
    }

    fn files() -> [(&'static str, &'static str); 2] {
        [("pt-BR", PT_BR), ("es-ES", ES_ES)]
    }

    #[test]
    fn every_text_used_has_a_translation() {
        let keys = used_keys();
        assert!(keys.len() > 100, "found only {} texts", keys.len());
        for (name, file) in files() {
            let have: HashSet<String> = parse(file).into_iter().map(|(k, _)| k).collect();
            let mut missing: Vec<String> = keys
                .iter()
                .filter(|(_, k)| !have.contains(k))
                .map(|(f, k)| format!("{f}: {k:?}"))
                .collect();
            missing.extend(
                listed_keys()
                    .into_iter()
                    .filter(|k| !have.contains(k))
                    .map(|k| format!("(listed): {k:?}")),
            );
            assert!(
                missing.is_empty(),
                "{name} is missing:\n{}",
                missing.join("\n")
            );
        }
    }

    #[test]
    fn no_translation_is_left_over() {
        let mut known: HashSet<String> = used_keys().into_iter().map(|(_, k)| k).collect();
        known.extend(listed_keys());
        known.extend(maker_keys());
        for (name, file) in files() {
            let mut seen = HashSet::new();
            for (k, v) in parse(file) {
                assert!(known.contains(&k), "{name}: {k:?} isn't used anywhere");
                assert!(seen.insert(k.clone()), "{name}: {k:?} is there twice");
                assert!(!v.is_empty(), "{name}: {k:?} has no translation");
                assert_eq!(
                    k.matches("{}").count(),
                    v.matches("{}").count(),
                    "{name}: {k:?} and {v:?} need the same blanks"
                );
            }
        }
    }

    #[test]
    fn every_line_parses() {
        for (name, file) in files() {
            for line in file.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                assert!(line.contains(" = "), "{name}: can't read {line:?}");
            }
        }
    }

    #[test]
    fn looking_up_and_filling_in() {
        assert_eq!(fill("Mods on: {}", &["3"]), "Mods on: 3");
        assert_eq!(fill("{} and {}", &["a"]), "a and ");
        assert_eq!(unescape(r"one\ntwo \\ three"), "one\ntwo \\ three");
        assert_eq!(
            Language::from_code("pt_BR.UTF-8"),
            Some(Language::PortugueseBrazil)
        );
        assert_eq!(Language::from_code("es-ES"), Some(Language::SpanishSpain));
        assert_eq!(Language::from_code("fr-FR"), None);
        assert_eq!(Language::English.next().next().next(), Language::English);
        let pt = Language::PortugueseBrazil.translations().unwrap();
        assert_eq!(pt.get("Play").copied(), Some("Jogar"));
        let es = Language::SpanishSpain.translations().unwrap();
        assert_eq!(es.get("Play").copied(), Some("Jugar"));
        // English has no table: everything is itself.
        assert!(Language::English.translations().is_none());
    }
}
