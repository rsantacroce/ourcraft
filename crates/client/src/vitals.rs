//! Survival on screen: hearts, hunger and breath above the hotbar, a red
//! flash when you get hurt. Also eating (hold "place" with food in hand)
//! and hitting animals and monsters (break on them).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, ServerMessage};

use crate::input::{Actions, UiFocus};
use crate::interaction::{PlayerInventory, TargetMob};
use crate::items::Items;
use crate::net::{Connection, FromServer, NetReceiveSet};

pub struct VitalsPlugin;

impl Plugin for VitalsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Vitals>()
            .add_systems(Startup, (make_icons, spawn_bars).chain())
            .add_systems(PreUpdate, receive_vitals.after(NetReceiveSet))
            .add_systems(Update, (draw_bars, hurt_flash, eat, hit))
            .add_systems(
                PreUpdate,
                forget_vitals.run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// Health and food out of 20, breath out of 12; all 0 = not shown.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct Vitals {
    pub health: f32,
    pub food: f32,
    pub air: f32,
}

impl Vitals {
    pub fn shown(&self) -> bool {
        self.health > 0.0 || self.food > 0.0
    }
}

#[derive(Resource)]
struct Icons {
    heart: Handle<Image>,
    half_heart: Handle<Image>,
    empty_heart: Handle<Image>,
    food: Handle<Image>,
    half_food: Handle<Image>,
    empty_food: Handle<Image>,
    bubble: Handle<Image>,
}

#[derive(Component)]
struct Bars;

#[derive(Component)]
struct Flash(f32);

/// 9×9 pixel art.
fn icon(rows: [&str; 9], palette: &[(char, [u8; 4])]) -> Image {
    let mut px = Vec::with_capacity(9 * 9 * 4);
    for row in rows {
        for c in row.chars() {
            px.extend_from_slice(
                &palette
                    .iter()
                    .find(|(k, _)| *k == c)
                    .map_or([0; 4], |(_, v)| *v),
            );
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: 9,
            height: 9,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

const HEART: [&str; 9] = [
    ".kk...kk.",
    "kRRk.kRRk",
    "kRLRkRRRk",
    "kRRRRRRRk",
    "kRRRRRRRk",
    ".kRRRRRk.",
    "..kRRRk..",
    "...kRk...",
    "....k....",
];
const HALF_HEART: [&str; 9] = [
    ".kk...kk.",
    "kRRk.kGGk",
    "kRLRkGGGk",
    "kRRRRGGGk",
    "kRRRRGGGk",
    ".kRRRGGk.",
    "..kRRGk..",
    "...kRk...",
    "....k....",
];
const APPLE: [&str; 9] = [
    "....s.g..",
    "....sgg..",
    ".kkkskkk.",
    "kRLRRRRRk",
    "kRLRRRRRk",
    "kRRRRRRRk",
    "kRRRRRRRk",
    ".kRRRRRk.",
    "..kk.kk..",
];
const HALF_APPLE: [&str; 9] = [
    "....s.g..",
    "....sgg..",
    ".kkkskkk.",
    "kRLRGGGGk",
    "kRLRGGGGk",
    "kRRRGGGGk",
    "kRRRGGGGk",
    ".kRRGGGk.",
    "..kk.kk..",
];
const BUBBLE: [&str; 9] = [
    "..bbbbb..",
    ".bWWlllb.",
    "bWWlllllb",
    "bWlllllb.",
    "blllllllb",
    "bllllllb.",
    "blllllllb",
    ".bllllb..",
    "..bbbbb..",
];

fn make_icons(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let k = ('k', [40, 20, 20, 255]);
    let grey = ('G', [70, 60, 60, 255]);
    let red = [
        k,
        ('R', [230, 40, 50, 255]),
        ('L', [255, 170, 170, 255]),
        grey,
    ];
    let empty = [k, ('R', [70, 60, 60, 255]), ('L', [90, 80, 80, 255]), grey];
    let apple = [
        k,
        ('R', [220, 60, 40, 255]),
        ('L', [255, 170, 140, 255]),
        ('s', [96, 64, 34, 255]),
        ('g', [90, 170, 60, 255]),
        grey,
    ];
    let empty_apple = [
        k,
        ('R', [70, 60, 60, 255]),
        ('L', [90, 80, 80, 255]),
        ('s', [70, 60, 60, 255]),
        ('g', [70, 60, 60, 255]),
    ];
    let bubble = [
        ('b', [40, 90, 170, 255]),
        ('l', [120, 190, 255, 255]),
        ('W', [235, 248, 255, 255]),
    ];
    let mut add = |rows, pal: &[(char, [u8; 4])]| images.add(icon(rows, pal));
    let icons = Icons {
        heart: add(HEART, &red),
        half_heart: add(HALF_HEART, &red),
        empty_heart: add(HEART, &empty),
        food: add(APPLE, &apple),
        half_food: add(HALF_APPLE, &apple),
        empty_food: add(APPLE, &empty_apple),
        bubble: add(BUBBLE, &bubble),
    };
    commands.insert_resource(icons);
}

fn spawn_bars(mut commands: Commands) {
    commands.spawn((
        Name::new("Vitals"),
        Bars,
        Node {
            position_type: PositionType::Absolute,
            bottom: px(12.0 + 52.0 + 28.0),
            width: percent(100),
            justify_content: JustifyContent::Center,
            column_gap: px(40),
            ..default()
        },
    ));
    commands.spawn((
        Name::new("Hurt flash"),
        Flash(0.0),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
    ));
}

fn receive_vitals(
    mut messages: MessageReader<FromServer>,
    mut vitals: ResMut<Vitals>,
    mut flash: Query<&mut Flash>,
) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::Vitals { health, food, air } = msg {
            let new = Vitals {
                health: *health,
                food: *food,
                air: *air,
            };
            if vitals.shown() && new.health < vitals.health {
                for mut f in &mut flash {
                    f.0 = 0.45;
                }
            }
            *vitals = new;
        }
    }
}

fn forget_vitals(mut vitals: ResMut<Vitals>) {
    *vitals = Vitals::default();
}

/// Ten icons for a value out of 20 (full, half or empty each).
fn row(
    parent: &mut ChildSpawnerCommands,
    value: f32,
    full: &Handle<Image>,
    half: &Handle<Image>,
    empty: &Handle<Image>,
) {
    parent
        .spawn(Node {
            column_gap: px(2),
            ..default()
        })
        .with_children(|r| {
            for i in 0..10 {
                let v = value - i as f32 * 2.0;
                let img = if v >= 2.0 {
                    full
                } else if v >= 1.0 {
                    half
                } else {
                    empty
                };
                r.spawn((
                    ImageNode::new(img.clone()),
                    Node {
                        width: px(18),
                        height: px(18),
                        ..default()
                    },
                ));
            }
        });
}

fn draw_bars(
    mut commands: Commands,
    vitals: Res<Vitals>,
    icons: Res<Icons>,
    tools: Res<crate::gamemode::ToolHotbar>,
    bars: Single<Entity, With<Bars>>,
) {
    if !vitals.is_changed() && !tools.is_changed() {
        return;
    }
    let mut bars = commands.entity(*bars);
    bars.despawn_children();
    if !vitals.shown() || tools.active() {
        return;
    }
    bars.with_children(|b| {
        row(
            b,
            vitals.health,
            &icons.heart,
            &icons.half_heart,
            &icons.empty_heart,
        );
        if vitals.air < crate::vitals::MAX_AIR {
            b.spawn(Node {
                column_gap: px(2),
                ..default()
            })
            .with_children(|r| {
                for _ in 0..(vitals.air / crate::vitals::MAX_AIR * 10.0).ceil() as usize {
                    r.spawn((
                        ImageNode::new(icons.bubble.clone()),
                        Node {
                            width: px(16),
                            height: px(16),
                            ..default()
                        },
                    ));
                }
            });
        }
        row(
            b,
            vitals.food,
            &icons.food,
            &icons.half_food,
            &icons.empty_food,
        );
    });
}

/// Breath, as the server counts it.
pub const MAX_AIR: f32 = 12.0;

fn hurt_flash(time: Res<Time>, mut flash: Query<(&mut Flash, &mut BackgroundColor)>) {
    for (mut f, mut bg) in &mut flash {
        f.0 = (f.0 - time.delta_secs()).max(0.0);
        bg.0 = Color::srgba(0.9, 0.1, 0.1, f.0 * 0.6);
    }
}

/// Seconds of holding "place" to eat.
const EAT_TIME: f32 = 0.9;

/// Hold "place" with food in hand to eat it.
#[allow(clippy::too_many_arguments)]
fn eat(
    time: Res<Time>,
    actions: Res<Actions>,
    focus: Res<UiFocus>,
    vitals: Res<Vitals>,
    items: Res<Items>,
    inventory: Res<PlayerInventory>,
    target: Res<crate::interaction::Target>,
    blocks: Res<crate::world::Blocks>,
    conn: Option<ResMut<Connection>>,
    mut sounds: MessageWriter<crate::audio::PlaySound>,
    mut held_for: Local<f32>,
) {
    let food = inventory
        .held()
        .and_then(|i| items.get(i).map(|d| (i, d.food, d.heals)))
        .filter(|(_, f, h)| *f > 0 || *h > 0);
    // Using a door or chest wins over eating.
    let using = target
        .0
        .is_some_and(|h| crate::interaction::usable(&blocks.0, h.id));
    let wants = (actions.eat || actions.place_block && !using)
        && !focus.gameplay_blocked()
        && vitals.shown();
    let Some((item, nutrition, heals)) = food.filter(|_| wants) else {
        *held_for = 0.0;
        return;
    };
    let hungry = vitals.food < 20.0 || heals > 0;
    if !hungry || nutrition == 0 && heals == 0 {
        return;
    }
    let before = *held_for;
    *held_for += time.delta_secs();
    // A crunch while chewing.
    if (before / 0.3).floor() != (*held_for / 0.3).floor() {
        sounds.write(crate::audio::PlaySound::ui("munch"));
    }
    if *held_for >= EAT_TIME {
        *held_for = 0.0;
        if let Some(mut conn) = conn {
            conn.send(Channel::Reliable, &ClientMessage::Eat { item: item.0 });
        }
    }
}

/// Left click on an animal or monster.
fn hit(
    time: Res<Time>,
    actions: Res<Actions>,
    focus: Res<UiFocus>,
    target: Res<TargetMob>,
    inventory: Res<PlayerInventory>,
    conn: Option<ResMut<Connection>>,
    mut cooldown: Local<f32>,
) {
    *cooldown -= time.delta_secs();
    let (Some((mob, _)), Some(mut conn)) = (&target.0, conn) else {
        return;
    };
    if !actions.break_block || focus.gameplay_blocked() || *cooldown > 0.0 {
        return;
    }
    *cooldown = 0.45;
    conn.send(
        Channel::Reliable,
        &ClientMessage::HitEntity {
            id: mob.id,
            held: inventory.held().unwrap_or(ItemId::NONE).0,
        },
    );
}
