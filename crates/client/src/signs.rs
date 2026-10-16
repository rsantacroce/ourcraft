//! Signs: writing on them, and showing what they say.
//!
//! The words are drawn with a tiny built-in 3×5 pixel font into a small
//! picture, which is put on the sign's board. Rude words are hidden by the
//! chat filter, like in chat.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::protocol::{Channel, ClientMessage, SIGN_LINE_LEN, SIGN_LINES, ServerMessage};
use ourcraft_common::shapes::{Dir4, VariantKind};

use crate::input::UiFocus;
use crate::inventory_ui::OpenScreen;
use crate::net::{Connection, FromServer, NetReceiveSet};
use crate::world::{Blocks, ClientWorld};

pub struct SignsPlugin;

impl Plugin for SignsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SignTexts>()
            .init_resource::<SignEditor>()
            .add_systems(PreUpdate, receive_signs.after(NetReceiveSet))
            .add_systems(Update, (edit_signs, draw_editor, place_sign_text).chain())
            .add_systems(
                PreUpdate,
                forget_signs.run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// What every sign we know of says, and the entity showing it.
#[derive(Resource, Default)]
struct SignTexts {
    lines: HashMap<IVec3, Vec<String>>,
    shown: HashMap<IVec3, (Entity, Vec<String>, u16)>,
}

/// The sign being written on.
#[derive(Resource, Default)]
struct SignEditor {
    pos: Option<IVec3>,
    lines: Vec<String>,
    line: usize,
}

#[derive(Component)]
struct EditorPanel;

/// 3×5 glyphs, one bit per pixel, top row first (most significant bit is
/// the top left pixel).
fn glyph(c: char) -> u16 {
    let rows: [u8; 5] = match c.to_ascii_uppercase() {
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [6, 1, 2, 4, 7],
        '3' => [6, 1, 2, 1, 6],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 6, 1, 6],
        '6' => [3, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 6],
        ' ' => [0, 0, 0, 0, 0],
        '!' => [2, 2, 2, 0, 2],
        '.' => [0, 0, 0, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        '\'' => [2, 2, 0, 0, 0],
        '"' => [5, 5, 0, 0, 0],
        '-' => [0, 0, 7, 0, 0],
        '+' => [0, 2, 7, 2, 0],
        ':' => [0, 2, 0, 2, 0],
        ';' => [0, 2, 0, 2, 4],
        '(' => [1, 2, 2, 2, 1],
        ')' => [4, 2, 2, 2, 4],
        '/' => [1, 1, 2, 4, 4],
        '=' => [0, 7, 0, 7, 0],
        '*' => [0, 5, 2, 5, 0],
        '#' => [5, 7, 5, 7, 5],
        '&' => [2, 5, 2, 5, 3],
        '_' => [0, 0, 0, 0, 7],
        '@' => [7, 5, 7, 4, 3],
        '<' => [1, 2, 4, 2, 1],
        '>' => [4, 2, 1, 2, 4],
        _ => [6, 1, 2, 0, 2], // '?'
    };
    rows.iter().fold(0, |acc, r| acc << 3 | *r as u16)
}

/// Texture size: 15 characters of 4 pixels, 4 lines of 6 pixels, plus a
/// border.
const TEX_W: u32 = 64;
const TEX_H: u32 = 32;

/// Draw the lines in dark ink on a see-through picture.
pub fn render_text(lines: &[String]) -> Vec<u8> {
    let mut px = vec![0u8; (TEX_W * TEX_H * 4) as usize];
    for (row, line) in lines.iter().take(SIGN_LINES).enumerate() {
        let chars: Vec<char> = line.chars().take(SIGN_LINE_LEN).collect();
        // Centre each line.
        let width = chars.len() as u32 * 4;
        let x0 = (TEX_W - width) / 2;
        let y0 = 3 + row as u32 * 7;
        for (i, c) in chars.iter().enumerate() {
            let g = glyph(*c);
            for gy in 0..5 {
                for gx in 0..3 {
                    if g >> (14 - (gy * 3 + gx)) & 1 == 1 {
                        let x = x0 + i as u32 * 4 + gx;
                        let y = y0 + gy;
                        let p = ((y * TEX_W + x) * 4) as usize;
                        px[p..p + 4].copy_from_slice(&[40, 28, 18, 255]);
                    }
                }
            }
        }
    }
    px
}

fn receive_signs(mut messages: MessageReader<FromServer>, mut signs: ResMut<SignTexts>) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::SignText { pos, lines } = msg {
            let pos = IVec3::from_array(*pos);
            if lines.iter().all(|l| l.trim().is_empty()) {
                signs.lines.remove(&pos);
            } else {
                signs.lines.insert(pos, lines.clone());
            }
        }
    }
}

fn forget_signs(mut commands: Commands, mut signs: ResMut<SignTexts>) {
    for (_, (e, _, _)) in signs.shown.drain() {
        commands.entity(e).despawn();
    }
    signs.lines.clear();
}

/// Put each sign's words on its board (and take them away when the sign
/// goes).
#[allow(clippy::too_many_arguments)]
fn place_sign_text(
    mut commands: Commands,
    mut signs: ResMut<SignTexts>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut quad: Local<Option<Handle<Mesh>>>,
) {
    let signs = &mut *signs;
    // Remove text whose sign changed, went away, or got new words.
    let gone: Vec<IVec3> = signs
        .shown
        .iter()
        .filter(|(pos, (_, lines, block))| {
            world.block(pos.x, pos.y, pos.z).0 != *block || signs.lines.get(pos) != Some(lines)
        })
        .map(|(pos, _)| *pos)
        .collect();
    for pos in gone {
        if let Some((e, _, _)) = signs.shown.remove(&pos) {
            commands.entity(e).despawn();
        }
    }
    let quad = quad
        .get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 0.5)))
        .clone();
    for (pos, lines) in &signs.lines {
        if signs.shown.contains_key(pos) {
            continue;
        }
        let block = world.block(pos.x, pos.y, pos.z);
        let Some(v) = blocks.0.get(block).variant.as_ref() else {
            continue;
        };
        // Where the board's front is, for a sign facing north; then turned.
        let (centre, size) = match v.kind {
            VariantKind::Sign => (
                Vec3::new(0.0, 12.5 / 16.0 - 0.5, 7.0 / 16.0 - 0.5 - 0.01),
                7.0 / 16.0,
            ),
            VariantKind::WallSign => (Vec3::new(0.0, 0.0, 14.0 / 16.0 - 0.5 - 0.01), 8.0 / 16.0),
            _ => continue,
        };
        let turns = match v.facing {
            Dir4::North => 0.0,
            Dir4::East => 1.0,
            Dir4::South => 2.0,
            Dir4::West => 3.0,
        };
        let spin = Quat::from_rotation_y(-turns * std::f32::consts::FRAC_PI_2);
        let shown: Vec<String> = lines.iter().map(|l| crate::profanity::filter(l)).collect();
        let mut image = Image::new(
            Extent3d {
                width: TEX_W,
                height: TEX_H,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            render_text(&shown),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            alpha_mode: AlphaMode::Mask(0.5),
            unlit: true,
            ..default()
        });
        let middle = pos.as_vec3() + Vec3::splat(0.5);
        let e = commands
            .spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(middle + spin * centre)
                    // The quad faces +z; turn it to face north, then the
                    // sign's way.
                    .with_rotation(spin * Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::new(1.0, size * 2.0, 1.0)),
            ))
            .id();
        signs.shown.insert(*pos, (e, lines.clone(), block.0));
    }
}

/// Something to do to the sign being written: from the keyboard, or from
/// the on-screen keys on touch screens.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum SignKey {
    Char(char),
    Back,
    Next,
    Done,
}

/// The on-screen keys, row by row.
const KEY_ROWS: [&str; 4] = ["1234567890", "QWERTYUIOP", "ASDFGHJKL!", "ZXCVBNM?.,"];

impl SignEditor {
    /// Returns true when the writing is finished.
    fn press(&mut self, key: SignKey) -> bool {
        let line = self.line;
        match key {
            SignKey::Next => {
                if line + 1 < SIGN_LINES {
                    self.line += 1;
                } else {
                    return true;
                }
            }
            SignKey::Done => return true,
            SignKey::Back => {
                if self.lines[line].pop().is_none() && line > 0 {
                    self.line -= 1;
                }
            }
            SignKey::Char(c) => {
                if self.lines[line].chars().count() < SIGN_LINE_LEN {
                    self.lines[line].push(c);
                }
            }
        }
        false
    }
}

/// Typing on the sign that's open.
#[allow(clippy::too_many_arguments)]
fn edit_signs(
    mut typed: MessageReader<KeyboardInput>,
    (mut clicks, mut touches, window): (
        MessageReader<bevy::input::mouse::MouseButtonInput>,
        MessageReader<bevy::input::touch::TouchInput>,
        Single<&Window, With<bevy::window::PrimaryWindow>>,
    ),
    keys: Query<(&SignKey, &ComputedNode, &UiGlobalTransform)>,
    lines: Query<(&EditorLine, &ComputedNode, &UiGlobalTransform)>,
    mut focus: ResMut<UiFocus>,
    screen: Res<OpenScreen>,
    signs: Res<SignTexts>,
    mut editor: ResMut<SignEditor>,
    conn: Option<ResMut<Connection>>,
) {
    let open = match *screen {
        OpenScreen::Sign(pos) if focus.inventory_open => Some(pos),
        _ => None,
    };
    if editor.pos != open {
        // Just opened: start from what it says now.
        if let Some(pos) = open {
            editor.lines = signs.lines.get(&pos).cloned().unwrap_or_default();
            editor.lines.resize(SIGN_LINES, String::new());
            editor.line = 0;
        }
        editor.pos = open;
        typed.clear();
        clicks.clear();
        touches.clear();
        return;
    }
    let Some(pos) = open else { return };
    let mut pressed: Vec<SignKey> = Vec::new();
    for e in typed.read().filter(|e| e.state == ButtonState::Pressed) {
        match &e.logical_key {
            Key::Enter | Key::ArrowDown => pressed.push(SignKey::Next),
            Key::ArrowUp => editor.line = editor.line.saturating_sub(1),
            Key::Escape => pressed.push(SignKey::Done),
            Key::Backspace => pressed.push(SignKey::Back),
            Key::Space => pressed.push(SignKey::Char(' ')),
            Key::Character(s) => pressed.extend(s.chars().map(SignKey::Char)),
            _ => {}
        }
    }
    // On-screen keys, and tapping a line moves the cursor there. We look
    // at where each click or touch starts ourselves (in physical pixels,
    // like the UI's layout), so even the quickest tap counts.
    let scale = window.scale_factor();
    let mut points: Vec<Vec2> = clicks
        .read()
        .filter(|c| c.state == ButtonState::Pressed && c.button == MouseButton::Left)
        .filter_map(|_| window.physical_cursor_position())
        .collect();
    points.extend(
        touches
            .read()
            .filter(|t| t.phase == bevy::input::touch::TouchPhase::Started)
            .map(|t| t.position * scale),
    );
    for point in points {
        if let Some((k, _, _)) = keys.iter().find(|(_, n, t)| n.contains_point(**t, point)) {
            pressed.push(*k);
        } else if let Some((l, _, _)) = lines.iter().find(|(_, n, t)| n.contains_point(**t, point))
        {
            editor.line = l.0;
        }
    }
    let mut done = false;
    for key in pressed {
        done |= editor.press(key);
    }
    if done {
        if let Some(mut conn) = conn {
            conn.send(
                Channel::Reliable,
                &ClientMessage::SetSign {
                    pos: pos.to_array(),
                    lines: editor.lines.clone(),
                },
            );
        }
        focus.inventory_open = false;
    }
}

/// One line of the sign in the editor.
#[derive(Component)]
struct EditorLine(usize);

/// Show the sign being written: built when it opens, then only the lines
/// change (so a finger on a key isn't lost while typing).
fn draw_editor(
    mut commands: Commands,
    editor: Res<SignEditor>,
    touch: Res<crate::touch::TouchControls>,
    existing: Query<Entity, With<EditorPanel>>,
    mut shown_for: Local<Option<IVec3>>,
    mut lines: Query<(&EditorLine, &mut Text, &mut BackgroundColor)>,
) {
    if !editor.is_changed() {
        return;
    }
    if *shown_for != editor.pos {
        *shown_for = editor.pos;
        for e in &existing {
            commands.entity(e).despawn();
        }
        if editor.pos.is_some() {
            spawn_editor(&mut commands, &editor, touch.enabled);
        }
        return;
    }
    for (l, mut text, mut bg) in &mut lines {
        let (t, c) = line_look(&editor, l.0);
        text.0 = t;
        bg.0 = c;
    }
}

fn line_look(editor: &SignEditor, i: usize) -> (String, Color) {
    let line = editor.lines.get(i).map_or("", |l| l.as_str());
    if i == editor.line {
        (format!("{line}_"), Color::srgba(1.0, 1.0, 1.0, 0.25))
    } else {
        (line.to_owned(), Color::srgba(1.0, 1.0, 1.0, 0.08))
    }
}

fn spawn_editor(commands: &mut Commands, editor: &SignEditor, keys: bool) {
    let key = |label: String, k: SignKey, width: f32| {
        (
            Button,
            k,
            Node {
                width: px(width),
                height: px(34),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.93, 0.88, 0.78)),
            children![(
                Text::new(label),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgb(0.2, 0.13, 0.06)),
                Pickable::IGNORE,
            )],
        )
    };
    commands
        .spawn((
            EditorPanel,
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(if keys { 5 } else { 8 }),
                        padding: UiRect::all(px(if keys { 12 } else { 20 })),
                        border_radius: BorderRadius::all(px(8)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.55, 0.4, 0.24)),
                ))
                .with_children(|board| {
                    board.spawn((
                        Text::new("Write on the sign"),
                        TextFont::from_font_size(18.0),
                    ));
                    for i in 0..SIGN_LINES {
                        let (t, c) = line_look(editor, i);
                        board.spawn((
                            Button,
                            EditorLine(i),
                            Text::new(t),
                            TextFont::from_font_size(if keys { 18.0 } else { 22.0 }),
                            TextColor(Color::srgb(0.15, 0.1, 0.05)),
                            Node {
                                min_width: px(260),
                                min_height: px(if keys { 20 } else { 28 }),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(c),
                        ));
                    }
                    if !keys {
                        board.spawn((
                            Text::new("Enter: next line (and done)   Esc: done"),
                            TextFont::from_font_size(13.0),
                            TextColor(Color::srgb(0.95, 0.9, 0.8)),
                        ));
                        return;
                    }
                    let row = || Node {
                        column_gap: px(4),
                        ..default()
                    };
                    for chars in KEY_ROWS {
                        board.spawn(row()).with_children(|r| {
                            for c in chars.chars() {
                                r.spawn(key(c.to_string(), SignKey::Char(c), 40.0));
                            }
                        });
                    }
                    board.spawn(row()).with_children(|r| {
                        r.spawn(key("Del".into(), SignKey::Back, 72.0));
                        r.spawn(key("Space".into(), SignKey::Char(' '), 160.0));
                        r.spawn(key("Next".into(), SignKey::Next, 96.0));
                        r.spawn(key("Done".into(), SignKey::Done, 72.0));
                    });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_with_keys() {
        let mut e = SignEditor {
            pos: None,
            lines: vec![String::new(); SIGN_LINES],
            line: 0,
        };
        for c in "HI".chars() {
            assert!(!e.press(SignKey::Char(c)));
        }
        e.press(SignKey::Next);
        e.press(SignKey::Char('!'));
        e.press(SignKey::Back);
        e.press(SignKey::Back); // empty line: back up to the one before
        assert_eq!((e.lines[0].as_str(), e.line), ("HI", 0));
        for _ in 0..30 {
            e.press(SignKey::Char('A'));
        }
        assert_eq!(e.lines[0].chars().count(), SIGN_LINE_LEN);
        for _ in 1..SIGN_LINES {
            assert!(!e.press(SignKey::Next));
        }
        assert!(e.press(SignKey::Next), "Next on the last line finishes");
    }

    #[test]
    fn every_key_has_a_glyph() {
        for c in KEY_ROWS.concat().chars() {
            assert_ne!(glyph(c), 0, "{c}");
        }
    }

    #[test]
    fn text_is_drawn_in_ink() {
        let px = render_text(&["HI".to_string()]);
        let ink = px.chunks(4).filter(|p| p[3] == 255).count();
        // H has 11 lit pixels and I has 9.
        assert_eq!(ink, 11 + 9);
        assert!(render_text(&[]).chunks(4).all(|p| p[3] == 0));
    }

    #[test]
    fn every_letter_and_digit_has_a_glyph() {
        let unknown = glyph('\u{1}');
        for c in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            assert_ne!(glyph(c), unknown, "{c}");
            assert_ne!(glyph(c), 0, "{c}");
        }
    }
}
