//! Survival messages from the server: items you picked up and furnace
//! screens. Also tells the server whether we're in creative or survival
//! (it decides whether broken blocks drop anything).

use bevy::prelude::*;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::protocol::{Channel, ClientMessage, ServerMessage};

use crate::input::UiFocus;
use crate::interaction::PlayerInventory;
use crate::inventory_ui::{ContainerView, Cursor, OpenScreen};
use crate::items::Items;
use crate::net::{Connection, FromServer, NetReceiveSet};
use crate::player::GameMode;

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, handle_messages.after(NetReceiveSet))
            .add_systems(Update, send_game_mode.run_if(resource_exists::<Connection>));
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_messages(
    mut messages: MessageReader<FromServer>,
    items: Res<Items>,
    mut inventory: ResMut<PlayerInventory>,
    mut cursor: ResMut<Cursor>,
    mut furnace: ResMut<ContainerView>,
    mut focus: ResMut<UiFocus>,
    screen: Res<OpenScreen>,
    conn: Option<ResMut<Connection>>,
) {
    let mut throw_back = Vec::new();
    for FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::GiveItems(stack) => {
                let stack = ItemStack::from(*stack);
                let left = inventory.add_stack(&items, stack);
                if left > 0 {
                    throw_back.push(ItemStack {
                        count: left,
                        ..stack
                    });
                }
            }
            ServerMessage::Container(view) => {
                if *screen == OpenScreen::Container(IVec3::from_array(view.pos))
                    && focus.inventory_open
                {
                    furnace.0 = Some(view.clone());
                }
            }
            ServerMessage::ContainerCursor(slot) => {
                cursor.pending = false;
                let stack = slot.map(ItemStack::from);
                let still_open =
                    focus.inventory_open && matches!(*screen, OpenScreen::Container(_));
                match (stack, still_open) {
                    (s, true) => cursor.stack = s,
                    // The screen closed meanwhile: into the inventory.
                    (Some(s), false) => {
                        let left = inventory.add_stack(&items, s);
                        if left > 0 {
                            throw_back.push(ItemStack { count: left, ..s });
                        }
                    }
                    (None, false) => {}
                }
            }
            ServerMessage::TakeItems(stack) => {
                // Take from the hand first, then anywhere else.
                let item = ourcraft_common::item::ItemId(stack.item);
                let mut left = stack.count;
                let selected = inventory.selected;
                let order = std::iter::once(selected)
                    .chain((0..inventory.slots.len()).filter(|i| *i != selected));
                for i in order.collect::<Vec<_>>() {
                    if left == 0 {
                        break;
                    }
                    if let Some(s) = inventory.slots[i].as_mut().filter(|s| s.item == item) {
                        let n = left.min(s.count);
                        s.count -= n;
                        left -= n;
                        if s.count == 0 {
                            inventory.slots[i] = None;
                        }
                    }
                }
            }
            ServerMessage::ContainerClosed => {
                if matches!(*screen, OpenScreen::Container(_)) {
                    focus.inventory_open = false;
                    furnace.0 = None;
                }
            }
            _ => {}
        }
    }
    if let Some(mut conn) = conn {
        for s in throw_back {
            conn.send(Channel::Reliable, &ClientMessage::DropItems(s.into()));
        }
    }
}

/// Tell the server our game mode whenever it changes (and once we've
/// joined).
fn send_game_mode(
    mode: Res<GameMode>,
    mut conn: ResMut<Connection>,
    mut sent: Local<Option<(bool, bool)>>,
) {
    let welcomed = conn.is_welcomed();
    let creative = *mode == GameMode::Creative;
    if !welcomed {
        *sent = None;
        return;
    }
    if *sent != Some((welcomed, creative)) {
        *sent = Some((welcomed, creative));
        conn.send(Channel::Reliable, &ClientMessage::GameMode { creative });
    }
}
