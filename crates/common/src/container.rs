//! Blocks that hold items: furnaces (and, later, chests).
//!
//! The server owns them. Players see the slots through a screen and move
//! stacks in and out; the server checks each move against the slot rules
//! here, so both sides agree on what's allowed.

use crate::inventory::ItemStack;
use crate::item::{ItemRegistry, SMELT_TIME};

/// Furnace slots.
pub const FURNACE_INPUT: usize = 0;
pub const FURNACE_FUEL: usize = 1;
pub const FURNACE_OUTPUT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContainerKind {
    Furnace,
    /// 27 slots for anything.
    Chest,
}

impl ContainerKind {
    pub fn slots(self) -> usize {
        match self {
            ContainerKind::Furnace => 3,
            ContainerKind::Chest => 27,
        }
    }

    /// May a player put `stack` into `slot`?
    pub fn accepts(self, items: &ItemRegistry, slot: usize, stack: &ItemStack) -> bool {
        match self {
            ContainerKind::Furnace => match slot {
                FURNACE_INPUT => items.smelt_result(stack.item).is_some(),
                FURNACE_FUEL => items.fuel(stack.item) > 0.0,
                _ => false, // the output is take-only
            },
            ContainerKind::Chest => slot < 27,
        }
    }
}

/// A container's contents and, for furnaces, how the fire is doing.
#[derive(Clone, Debug, PartialEq)]
pub struct Container {
    pub kind: ContainerKind,
    pub slots: Vec<Option<ItemStack>>,
    /// Seconds of fire left from the last fuel item.
    pub burn_left: f32,
    /// Seconds that fuel item lasted in total (for the flame picture).
    pub burn_total: f32,
    /// Seconds spent smelting the current item.
    pub progress: f32,
}

impl Container {
    pub fn new(kind: ContainerKind) -> Self {
        Self {
            kind,
            slots: vec![None; kind.slots()],
            burn_left: 0.0,
            burn_total: 0.0,
            progress: 0.0,
        }
    }

    /// Advance a furnace by `dt` seconds. Returns true if anything changed
    /// that viewers should see.
    pub fn tick(&mut self, items: &ItemRegistry, dt: f32) -> bool {
        if self.kind != ContainerKind::Furnace {
            return false;
        }
        let result = self.slots[FURNACE_INPUT].and_then(|s| items.smelt_result(s.item));
        let output_ok = |out: &Option<ItemStack>, r| match out {
            None => true,
            Some(o) => o.item == r && o.count < items.max_stack(r),
        };
        let can_smelt = result.is_some_and(|r| output_ok(&self.slots[FURNACE_OUTPUT], r));
        let was_burning = self.burn_left > 0.0;
        let mut changed = false;

        // Light a new piece of fuel when there's something to smelt.
        if self.burn_left <= 0.0
            && can_smelt
            && let Some(fuel) = self.slots[FURNACE_FUEL]
        {
            let secs = items.fuel(fuel.item);
            if secs > 0.0 {
                self.burn_left = secs;
                self.burn_total = secs;
                let left = fuel.count - 1;
                self.slots[FURNACE_FUEL] = (left > 0).then_some(ItemStack {
                    count: left,
                    ..fuel
                });
                changed = true;
            }
        }
        if self.burn_left > 0.0 {
            self.burn_left = (self.burn_left - dt).max(0.0);
            if can_smelt {
                self.progress += dt;
                if self.progress >= SMELT_TIME {
                    self.progress = 0.0;
                    let r = result.expect("can_smelt implies a result");
                    let input = self.slots[FURNACE_INPUT].expect("has input");
                    self.slots[FURNACE_INPUT] = (input.count > 1).then_some(ItemStack {
                        count: input.count - 1,
                        ..input
                    });
                    self.slots[FURNACE_OUTPUT] = Some(match self.slots[FURNACE_OUTPUT] {
                        Some(o) => ItemStack {
                            count: o.count + 1,
                            ..o
                        },
                        None => ItemStack::new(r, 1),
                    });
                }
                changed = true;
            }
        }
        if !can_smelt && self.progress > 0.0 {
            self.progress = 0.0;
            changed = true;
        }
        changed || was_burning != (self.burn_left > 0.0)
    }

    /// 0..1 for the progress arrow.
    pub fn smelt_fraction(&self) -> f32 {
        (self.progress / SMELT_TIME).clamp(0.0, 1.0)
    }

    /// 0..1 for the flame.
    pub fn burn_fraction(&self) -> f32 {
        if self.burn_total > 0.0 {
            (self.burn_left / self.burn_total).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockRegistry, blocks};
    use crate::item::ItemId;

    #[test]
    fn furnace_smelts_ore_with_coal() {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let ore = ItemId::from_block(blocks::IRON_ORE);
        let coal = items.by_name("core:coal").unwrap();
        let ingot = items.by_name("core:iron_ingot").unwrap();
        let mut f = Container::new(ContainerKind::Furnace);
        assert!(
            f.kind
                .accepts(&items, FURNACE_INPUT, &ItemStack::new(ore, 1))
        );
        assert!(
            !f.kind
                .accepts(&items, FURNACE_INPUT, &ItemStack::new(coal, 1))
        );
        assert!(
            f.kind
                .accepts(&items, FURNACE_FUEL, &ItemStack::new(coal, 1))
        );
        assert!(
            !f.kind
                .accepts(&items, FURNACE_OUTPUT, &ItemStack::new(ingot, 1))
        );

        f.slots[FURNACE_INPUT] = Some(ItemStack::new(ore, 3));
        f.slots[FURNACE_FUEL] = Some(ItemStack::new(coal, 1));
        // Nothing happens with no time passing… then the coal catches.
        for _ in 0..(3.0 * SMELT_TIME / 0.05) as usize + 2 {
            f.tick(&items, 0.05);
        }
        assert_eq!(f.slots[FURNACE_OUTPUT], Some(ItemStack::new(ingot, 3)));
        assert_eq!(f.slots[FURNACE_INPUT], None);
        assert_eq!(f.slots[FURNACE_FUEL], None, "the coal was used");
        assert!(f.burn_left > 0.0, "and is still burning");
    }

    #[test]
    fn no_fuel_no_fire() {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let mut f = Container::new(ContainerKind::Furnace);
        f.slots[FURNACE_INPUT] = Some(ItemStack::new(ItemId::from_block(blocks::SAND), 1));
        for _ in 0..200 {
            f.tick(&items, 0.1);
        }
        assert_eq!(f.slots[FURNACE_OUTPUT], None);
    }
}
