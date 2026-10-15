//! Chat: shows recent messages and (with a keyboard) lets you type.
//!
//! Press Enter to start typing, Enter again to send, Esc to cancel. Messages
//! fade out after a while. Commands start with `/` (try `/help`).

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use ourcraft_common::protocol::{Channel, ClientMessage, MAX_CHAT_LEN, ServerMessage};

use crate::input::UiFocus;
use crate::net::{Connection, FromServer, NetReceiveSet};

/// Seconds a message stays on screen.
const SHOW_SECS: f64 = 12.0;
const MAX_LINES: usize = 8;

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>()
            .init_resource::<ChatInput>()
            .add_systems(Startup, spawn_chat_ui)
            .add_systems(PreUpdate, collect_messages.after(NetReceiveSet))
            .add_systems(Update, (type_chat, update_chat_ui).chain())
            .add_systems(
                PreUpdate,
                (|mut log: ResMut<ChatLog>| log.lines.clear())
                    .run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// A line of chat. `from` is `None` for server announcements.
#[derive(Debug, Clone)]
pub struct ChatLine {
    pub from: Option<String>,
    pub text: String,
    /// When it arrived (seconds since start).
    pub at: f64,
}

#[derive(Resource, Default, Debug)]
pub struct ChatLog {
    pub lines: Vec<ChatLine>,
}

impl ChatLog {
    pub fn push(&mut self, from: Option<String>, text: String, at: f64) {
        self.lines.push(ChatLine { from, text, at });
        if self.lines.len() > 100 {
            self.lines.remove(0);
        }
    }
}

/// What's being typed, while the chat box is open.
#[derive(Resource, Default, Debug)]
pub struct ChatInput {
    pub text: String,
}

fn collect_messages(
    time: Res<Time<Real>>,
    mut messages: MessageReader<FromServer>,
    mut log: ResMut<ChatLog>,
) {
    let now = time.elapsed_secs_f64();
    for FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::Chat { from, text } => log.push(from.clone(), text.clone(), now),
            ServerMessage::Welcome {
                server_name, motd, ..
            } => {
                log.push(None, format!("Welcome to {server_name}! {motd}"), now);
            }
            _ => {}
        }
    }
}

fn type_chat(
    mut typed: MessageReader<KeyboardInput>,
    mut focus: ResMut<UiFocus>,
    mut input: ResMut<ChatInput>,
    conn: Option<ResMut<Connection>>,
) {
    // Opening and typing both read the same key events, so one key press
    // can never count twice.
    let presses: Vec<Key> = typed
        .read()
        .filter(|e| e.state == ButtonState::Pressed)
        .map(|e| e.logical_key.clone())
        .collect();
    let mut presses = presses.into_iter();
    if !focus.chat_open {
        if focus.inventory_open || focus.menu_open {
            return;
        }
        // Enter opens chat; `/` opens it ready for a command.
        let Some(opener) = presses.find(|k| {
            matches!(k, Key::Enter) || matches!(k, Key::Character(c) if c.as_str() == "/")
        }) else {
            return;
        };
        focus.chat_open = true;
        input.text.clear();
        if matches!(opener, Key::Character(_)) {
            input.text.push('/');
        }
    }
    let mut conn = conn;
    for key in presses {
        match &key {
            Key::Enter => {
                let text = input.text.trim().to_owned();
                if !text.is_empty()
                    && let Some(conn) = conn.as_mut()
                {
                    conn.send(Channel::Reliable, &ClientMessage::Chat { text });
                }
                focus.chat_open = false;
                input.text.clear();
                return;
            }
            Key::Escape => {
                focus.chat_open = false;
                input.text.clear();
                return;
            }
            Key::Backspace => {
                input.text.pop();
            }
            Key::Space => input.text.push(' '),
            Key::Character(s) if input.text.chars().count() < MAX_CHAT_LEN => {
                input.text.push_str(s);
            }
            _ => {}
        }
    }
}

#[derive(Component)]
struct ChatLines;

#[derive(Component)]
struct ChatBox;

fn spawn_chat_ui(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(10),
            bottom: px(90),
            width: px(520),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        })
        .with_children(|ui| {
            ui.spawn((
                ChatLines,
                Text::new(""),
                TextFont::from_font_size(15.0),
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::splat(1.0),
                    color: Color::BLACK,
                },
            ));
            ui.spawn((
                ChatBox,
                Text::new(""),
                TextFont::from_font_size(15.0),
                TextColor(Color::WHITE),
                Node {
                    padding: UiRect::axes(px(6), px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Visibility::Hidden,
            ));
        });
}

/// Format a chat line for display, optionally hiding rude words.
pub fn format_line(line: &ChatLine, filter: bool) -> String {
    let text = if filter {
        crate::profanity::filter(&line.text)
    } else {
        line.text.clone()
    };
    match &line.from {
        Some(name) => format!("<{name}> {text}"),
        None => format!("* {text}"),
    }
}

fn update_chat_ui(
    time: Res<Time<Real>>,
    settings: Res<crate::settings::ClientSettings>,
    log: Res<ChatLog>,
    input: Res<ChatInput>,
    focus: Res<UiFocus>,
    mut lines: Single<&mut Text, (With<ChatLines>, Without<ChatBox>)>,
    mut chat_box: Single<(&mut Text, &mut Visibility), (With<ChatBox>, Without<ChatLines>)>,
) {
    let now = time.elapsed_secs_f64();
    // While typing, show more history; otherwise only recent lines.
    let recent: Vec<String> = log
        .lines
        .iter()
        .filter(|l| focus.chat_open || now - l.at < SHOW_SECS)
        .map(|l| format_line(l, settings.chat_filter))
        .collect();
    let start = recent.len().saturating_sub(MAX_LINES);
    lines.0 = recent[start..].join("\n");

    let (text, vis) = &mut *chat_box;
    if focus.chat_open {
        text.0 = format!("> {}_", input.text);
        **vis = Visibility::Inherited;
    } else {
        **vis = Visibility::Hidden;
    }
}
