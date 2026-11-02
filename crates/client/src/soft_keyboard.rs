//! The on-screen keyboard on phones and tablets.
//!
//! While chat is open or a menu field (world name, seed, address, your
//! name) is being typed into, the system keyboard slides up; it goes away
//! again when you finish. Desktop keyboards are left alone: turning on
//! IME there would change how ordinary key presses arrive.
//!
//! What the keyboard types ends up as ordinary [`KeyboardInput`] messages,
//! so chat and the menus read it like any other keyboard:
//!
//! - **iOS:** winit already turns typing into key presses. Return comes
//!   through as a `"\n"` character, which [`key_of`] turns into Enter.
//! - **Android:** GameActivity keeps what's typed in a text box of its own
//!   (winit drops its text events), so each frame we look at that text and
//!   send the difference on as key presses: new characters, and Backspace
//!   for any taken away. The keyboard is set up as multi-line with no
//!   action button, so its Enter key arrives as a newline (or a real Enter
//!   key press), never as a "Done" action we can't see.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
#[cfg(any(target_os = "ios", target_os = "android"))]
use bevy::window::PrimaryWindow;

use crate::input::UiFocus;

pub struct SoftKeyboardPlugin;

impl Plugin for SoftKeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextFocus>();
        #[cfg(any(target_os = "ios", target_os = "android"))]
        app.add_systems(Last, show_keyboard);
        #[cfg(target_os = "android")]
        app.init_resource::<android::Typed>().add_systems(
            PreUpdate,
            android::read_typing.after(bevy::input::InputSystems),
        );
    }
}

/// Whether something outside the game itself wants text (set by the
/// menus; chat uses [`UiFocus::chat_open`]).
#[derive(Resource, Default, Debug)]
pub struct TextFocus {
    pub menu_field: bool,
}

/// Whether the keyboard should be up.
pub fn wants_text(focus: &UiFocus, text: &TextFocus) -> bool {
    focus.chat_open || text.menu_field
}

/// The key a key press stands for, treating a typed newline (the iOS
/// keyboard's Return) as Enter.
pub fn key_of(e: &KeyboardInput) -> Key {
    match &e.logical_key {
        Key::Character(s) if s.as_str() == "\n" || s.as_str() == "\r" => Key::Enter,
        k => k.clone(),
    }
}

#[cfg(any(target_os = "ios", target_os = "android"))]
fn show_keyboard(
    focus: Res<UiFocus>,
    text: Res<TextFocus>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let want = wants_text(&focus, &text);
    if window.ime_enabled != want {
        #[cfg(target_os = "android")]
        if want {
            android::prepare();
        }
        // winit shows the keyboard on iOS and Android when IME is allowed.
        window.ime_enabled = want;
    }
}

/// What changed between two versions of the typed text: how many
/// characters were taken off the end, then what was added.
pub fn text_change(old: &str, new: &str) -> (usize, String) {
    let same = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = old.chars().count() - same;
    (removed, new.chars().skip(same).collect())
}

/// Key presses for a change in the typed text.
pub fn presses_for(removed: usize, added: &str) -> Vec<Key> {
    let mut keys = vec![Key::Backspace; removed];
    for c in added.chars() {
        keys.push(match c {
            '\n' | '\r' => Key::Enter,
            ' ' => Key::Space,
            c => Key::Character(c.to_string().into()),
        });
    }
    keys
}

/// A made-up key press, as if from a keyboard.
pub fn press(key: Key, window: Entity) -> KeyboardInput {
    let text = match &key {
        Key::Character(s) => Some(s.clone()),
        Key::Space => Some(" ".into()),
        _ => None,
    };
    let key_code = match key {
        Key::Enter => KeyCode::Enter,
        Key::Backspace => KeyCode::Backspace,
        Key::Space => KeyCode::Space,
        _ => KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
    };
    KeyboardInput {
        key_code,
        logical_key: key,
        state: bevy::input::ButtonState::Pressed,
        text,
        repeat: false,
        window,
    }
}

#[cfg(target_os = "android")]
mod android {
    use bevy::android::ANDROID_APP;
    use bevy::android::android_activity::input::{
        ImeOptions, InputType, TextInputAction, TextInputState,
    };

    use super::*;

    /// The text GameActivity held last frame.
    #[derive(Resource, Default)]
    pub struct Typed(String);

    /// Before the keyboard comes up: start from an empty box, and ask for
    /// a keyboard whose Enter key types a newline.
    pub fn prepare() {
        let Some(app) = ANDROID_APP.get() else { return };
        app.set_ime_editor_info(
            InputType::TYPE_CLASS_TEXT
                | InputType::TYPE_TEXT_FLAG_MULTI_LINE
                | InputType::TYPE_TEXT_FLAG_NO_SUGGESTIONS,
            TextInputAction::None,
            ImeOptions::IME_FLAG_NO_ENTER_ACTION | ImeOptions::IME_FLAG_NO_FULLSCREEN,
        );
        app.set_text_input_state(TextInputState::default());
    }

    pub fn read_typing(
        focus: Res<UiFocus>,
        text: Res<TextFocus>,
        window: Single<Entity, With<PrimaryWindow>>,
        mut typed: ResMut<Typed>,
        mut keys: MessageWriter<KeyboardInput>,
    ) {
        if !wants_text(&focus, &text) {
            typed.0.clear();
            return;
        }
        let Some(app) = ANDROID_APP.get() else { return };
        let now = app.text_input_state().text;
        if now == typed.0 {
            return;
        }
        let (removed, added) = text_change(&typed.0, &now);
        for key in presses_for(removed, &added) {
            keys.write(press(key, *window));
        }
        // After Enter the box starts again from empty.
        if added.contains(['\n', '\r']) {
            app.set_text_input_state(TextInputState::default());
            typed.0.clear();
        } else {
            typed.0 = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_more_adds_the_new_letters() {
        assert_eq!(text_change("hel", "hello"), (0, "lo".into()));
        assert_eq!(text_change("", "hi"), (0, "hi".into()));
    }

    #[test]
    fn deleting_and_autocorrect_become_backspaces() {
        assert_eq!(text_change("hello", "hel"), (2, String::new()));
        // "teh" corrected to "the".
        assert_eq!(text_change("teh", "the"), (2, "he".into()));
        assert_eq!(text_change("héllo", "hé"), (3, String::new()));
    }

    #[test]
    fn newlines_are_enter() {
        assert_eq!(
            presses_for(1, "a b\n"),
            vec![
                Key::Backspace,
                Key::Character("a".into()),
                Key::Space,
                Key::Character("b".into()),
                Key::Enter,
            ]
        );
        let ios_return = press(Key::Character("\n".into()), Entity::PLACEHOLDER);
        assert_eq!(key_of(&ios_return), Key::Enter);
    }
}
