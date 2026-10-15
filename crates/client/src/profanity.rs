//! A simple chat filter for younger players (on by default; switch it off
//! in Settings). Words on the list are replaced with asterisks, including
//! common disguises like "sh1t" or "f.u.c.k".
//!
//! It's deliberately small and only catches the obvious: it's a filter for
//! a family game, not a moderation system.

/// Lowercase roots to mask. A word is masked if, after undoing look-alike
/// characters, it *starts with* one of these (so plurals and "-ing" forms
/// are caught too).
const BLOCKED: &[&str] = &[
    "fuck", "fuk", "shit", "bitch", "bastard", "asshole", "arsehole", "dick", "cock", "pussy",
    "cunt", "wank", "twat", "slut", "whore", "piss", "crap", "damn", "bollock", "prick", "retard",
    "fag", "nigg", "dumbass", "jackass",
];

/// Short words that would cause false alarms as prefixes ("ass" in
/// "assemble", "hell" in "hello"), so they only match exactly.
const BLOCKED_EXACT: &[&str] = &["ass", "arse", "hell", "tit", "tits", "wtf", "stfu"];

/// Undo common character swaps: 0→o, 1→i, 3→e, 4→a, 5→s, 7→t, @→a, $→s.
fn normalize(word: &str) -> String {
    word.chars()
        .filter_map(|c| {
            let c = c.to_ascii_lowercase();
            Some(match c {
                '0' => 'o',
                '1' | '!' | '|' => 'i',
                '3' => 'e',
                '4' | '@' => 'a',
                '5' | '$' => 's',
                '7' => 't',
                c if c.is_alphabetic() => c,
                // Dots, dashes, stars… inside a word are disguises: drop them.
                _ => return None,
            })
        })
        .collect()
}

fn is_blocked(word: &str) -> bool {
    let n = normalize(word);
    if n.len() < 3 {
        return false;
    }
    // Squash repeated letters too ("fuuuck").
    let mut squashed = String::new();
    for c in n.chars() {
        if !squashed.ends_with(c) {
            squashed.push(c);
        }
    }
    BLOCKED
        .iter()
        .any(|b| n.starts_with(b) || squashed.starts_with(b))
        || BLOCKED_EXACT.iter().any(|b| n == *b || squashed == *b)
}

/// Replace blocked words with asterisks, keeping everything else as is.
pub fn filter(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if is_blocked(word) {
            out.extend(std::iter::repeat_n('*', word.chars().count()));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_bad_words_and_disguises() {
        assert_eq!(filter("oh shit"), "oh ****");
        assert_eq!(filter("SH1T happens"), "**** happens");
        assert_eq!(filter("f.u.c.k off"), "******* off");
        assert_eq!(filter("fuuuuck"), "*******");
        assert_eq!(filter("you ass"), "you ***");
    }

    #[test]
    fn leaves_innocent_words_alone() {
        for ok in [
            "hello",
            "assemble",
            "class",
            "Scunthorpe",
            "shell",
            "title",
            "grass",
        ] {
            assert_eq!(filter(ok), ok, "{ok} was masked");
        }
        assert_eq!(filter("Nice build, Sam!"), "Nice build, Sam!");
    }
}
