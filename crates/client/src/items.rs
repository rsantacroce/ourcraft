//! The item list and crafting recipes, as sent by the server when we join
//! (mods can add to both). Before that, the built-in ones.

use std::sync::Arc;

use bevy::prelude::*;
use ourcraft_common::block::BlockRegistry;
use ourcraft_common::item::ItemRegistry;
use ourcraft_common::protocol::ServerMessage;
use ourcraft_common::recipes::{RecipeBook, builtin_recipes};

use crate::net::{FromServer, NetReceiveSet};

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let recipes = RecipeBook::new(&builtin_recipes(), &items);
        app.insert_resource(Items(Arc::new(items)))
            .insert_resource(Recipes(Arc::new(recipes)))
            .add_systems(PreUpdate, apply_items.after(NetReceiveSet));
    }
}

/// Every item (blocks first, with the same numbers).
#[derive(Resource, Clone, Deref)]
pub struct Items(pub Arc<ItemRegistry>);

#[derive(Resource, Clone, Deref)]
pub struct Recipes(pub Arc<RecipeBook>);

fn apply_items(mut commands: Commands, mut messages: MessageReader<FromServer>) {
    for FromServer(msg) in messages.read() {
        let ServerMessage::Welcome { content, .. } = msg else {
            continue;
        };
        let items = ItemRegistry::from_defs(content.items.clone());
        let recipes = RecipeBook::new(&content.recipes, &items);
        commands.insert_resource(Recipes(Arc::new(recipes)));
        commands.insert_resource(Items(Arc::new(items)));
    }
}
