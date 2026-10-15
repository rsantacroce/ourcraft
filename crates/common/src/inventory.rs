//! A simple inventory: 36 slots of item stacks. Slots 0–8 are the hotbar.

use crate::item::{ItemId, ItemRegistry};

pub const HOTBAR_SLOTS: usize = 9;
pub const INVENTORY_SLOTS: usize = 36;
pub const MAX_STACK: u16 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub item: ItemId,
    pub count: u16,
    /// How worn a tool is (uses so far); 0 for everything else.
    pub wear: u16,
}

impl ItemStack {
    pub fn new(item: ItemId, count: u16) -> Self {
        Self {
            item,
            count,
            wear: 0,
        }
    }

    /// Can `other` be added onto this stack?
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.item == other.item && self.wear == 0 && other.wear == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; INVENTORY_SLOTS],
    /// Selected hotbar slot, `0..HOTBAR_SLOTS`.
    pub selected: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: [None; INVENTORY_SLOTS],
            selected: 0,
        }
    }
}

impl Inventory {
    pub fn selected_stack(&self) -> Option<ItemStack> {
        self.slots[self.selected]
    }

    /// The item in hand, if any.
    pub fn held(&self) -> Option<ItemId> {
        self.selected_stack().map(|s| s.item)
    }

    /// Add items, filling existing stacks first, then empty slots (hotbar
    /// first). Returns how many didn't fit.
    pub fn add(&mut self, items: &ItemRegistry, item: ItemId, count: u16) -> u16 {
        self.add_stack(items, ItemStack::new(item, count))
    }

    /// Add a stack (which may be a worn tool). Returns how many didn't fit.
    pub fn add_stack(&mut self, items: &ItemRegistry, stack: ItemStack) -> u16 {
        let max = items.max_stack(stack.item);
        let mut count = stack.count;
        for slot in self.slots.iter_mut().flatten() {
            if slot.stacks_with(&stack) && slot.count < max {
                let n = count.min(max - slot.count);
                slot.count += n;
                count -= n;
                if count == 0 {
                    return 0;
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                let n = count.min(max);
                *slot = Some(ItemStack { count: n, ..stack });
                count -= n;
                if count == 0 {
                    return 0;
                }
            }
        }
        count
    }

    /// How many of `item` would fit.
    pub fn room_for(&self, items: &ItemRegistry, item: ItemId) -> u32 {
        let max = items.max_stack(item) as u32;
        self.slots
            .iter()
            .map(|s| match s {
                None => max,
                Some(s) if s.item == item && s.wear == 0 => max.saturating_sub(s.count as u32),
                Some(_) => 0,
            })
            .sum()
    }

    /// Use one item from the selected hotbar slot, returning it.
    pub fn take_selected(&mut self) -> Option<ItemId> {
        let slot = &mut self.slots[self.selected];
        let stack = slot.as_mut()?;
        let item = stack.item;
        stack.count -= 1;
        if stack.count == 0 {
            *slot = None;
        }
        Some(item)
    }

    /// Wear down the tool in hand by one use. Returns true if it broke.
    pub fn wear_selected(&mut self, items: &ItemRegistry) -> bool {
        let slot = &mut self.slots[self.selected];
        let Some(stack) = slot.as_mut() else {
            return false;
        };
        let durability = items.get(stack.item).map_or(0, |d| d.durability);
        if durability == 0 {
            return false;
        }
        stack.wear += 1;
        if stack.wear >= durability {
            *slot = None;
            return true;
        }
        false
    }

    pub fn select(&mut self, slot: usize) {
        self.selected = slot.min(HOTBAR_SLOTS - 1);
    }

    /// Move the selection by `delta` slots, wrapping around the hotbar.
    pub fn scroll(&mut self, delta: i32) {
        let n = HOTBAR_SLOTS as i32;
        self.selected = (self.selected as i32 + delta).rem_euclid(n) as usize;
    }

    pub fn swap(&mut self, a: usize, b: usize) {
        self.slots.swap(a, b);
    }

    /// Total count of an item across all slots.
    pub fn count(&self, item: ItemId) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| s.count as u32)
            .sum()
    }
}

/// Clicking a slot while maybe holding a stack on the mouse cursor, like
/// every inventory screen does it:
/// - **main click**: pick the stack up, put yours down, merge them, or swap;
/// - **secondary click**: put one item down, or pick up half.
///
/// Returns the new (slot, cursor).
pub fn click_slot(
    items: &ItemRegistry,
    slot: Option<ItemStack>,
    cursor: Option<ItemStack>,
    secondary: bool,
) -> (Option<ItemStack>, Option<ItemStack>) {
    match (slot, cursor, secondary) {
        (None, None, _) => (None, None),
        // Pick up all, or half.
        (Some(s), None, false) => (None, Some(s)),
        (Some(s), None, true) => {
            let take = s.count.div_ceil(2);
            let left = s.count - take;
            (
                (left > 0).then_some(ItemStack { count: left, ..s }),
                Some(ItemStack { count: take, ..s }),
            )
        }
        // Put down all, or one.
        (None, Some(c), false) => (Some(c), None),
        (None, Some(c), true) => {
            let rest = c.count - 1;
            (
                Some(ItemStack { count: 1, ..c }),
                (rest > 0).then_some(ItemStack { count: rest, ..c }),
            )
        }
        (Some(s), Some(c), secondary) if s.stacks_with(&c) => {
            let max = items.max_stack(s.item);
            let want = if secondary { 1 } else { c.count };
            let n = want.min(max.saturating_sub(s.count));
            let rest = c.count - n;
            (
                Some(ItemStack {
                    count: s.count + n,
                    ..s
                }),
                (rest > 0).then_some(ItemStack { count: rest, ..c }),
            )
        }
        // Different items: swap.
        (Some(s), Some(c), _) => (Some(c), Some(s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockRegistry, blocks};

    fn items() -> ItemRegistry {
        ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[])
    }
    fn b(id: crate::block::BlockId) -> ItemId {
        ItemId::from_block(id)
    }

    #[test]
    fn add_stacks_then_spills() {
        let items = items();
        let mut inv = Inventory::default();
        assert_eq!(inv.add(&items, b(blocks::DIRT), 100), 0);
        assert_eq!(inv.slots[0], Some(ItemStack::new(b(blocks::DIRT), 64)));
        assert_eq!(inv.slots[1], Some(ItemStack::new(b(blocks::DIRT), 36)));
        assert_eq!(inv.add(&items, b(blocks::DIRT), 10), 0);
        assert_eq!(inv.slots[1].unwrap().count, 46);
        assert_eq!(inv.count(b(blocks::DIRT)), 110);
    }

    #[test]
    fn full_inventory_returns_leftovers() {
        let items = items();
        let mut inv = Inventory::default();
        let capacity = INVENTORY_SLOTS as u16 * MAX_STACK;
        assert_eq!(inv.add(&items, b(blocks::STONE), capacity + 5), 5);
        assert_eq!(inv.room_for(&items, b(blocks::STONE)), 0);
    }

    #[test]
    fn tools_dont_stack_and_wear_out() {
        let items = items();
        let pick = items.by_name("core:wood_pickaxe").unwrap();
        let mut inv = Inventory::default();
        inv.add(&items, pick, 2);
        assert_eq!(inv.slots[0].unwrap().count, 1);
        assert_eq!(inv.slots[1].unwrap().count, 1);
        let uses = items.get(pick).unwrap().durability;
        for _ in 0..uses - 1 {
            assert!(!inv.wear_selected(&items));
        }
        assert!(inv.wear_selected(&items), "breaks on the last use");
        assert_eq!(inv.slots[0], None);
    }

    #[test]
    fn take_selected_empties_slot() {
        let items = items();
        let mut inv = Inventory::default();
        inv.add(&items, b(blocks::SAND), 2);
        assert_eq!(inv.take_selected(), Some(b(blocks::SAND)));
        assert_eq!(inv.take_selected(), Some(b(blocks::SAND)));
        assert_eq!(inv.take_selected(), None);
        assert_eq!(inv.slots[0], None);
    }

    #[test]
    fn scroll_wraps() {
        let mut inv = Inventory::default();
        inv.scroll(-1);
        assert_eq!(inv.selected, 8);
        inv.scroll(2);
        assert_eq!(inv.selected, 1);
    }

    #[test]
    fn clicking_slots() {
        let items = items();
        let dirt = |n| Some(ItemStack::new(b(blocks::DIRT), n));
        let sand = Some(ItemStack::new(b(blocks::SAND), 3));
        // Pick up, put down.
        assert_eq!(click_slot(&items, dirt(10), None, false), (None, dirt(10)));
        assert_eq!(click_slot(&items, None, dirt(10), false), (dirt(10), None));
        // Half and one.
        assert_eq!(click_slot(&items, dirt(5), None, true), (dirt(2), dirt(3)));
        assert_eq!(click_slot(&items, None, dirt(5), true), (dirt(1), dirt(4)));
        // Merge up to a full stack.
        assert_eq!(
            click_slot(&items, dirt(60), dirt(10), false),
            (dirt(64), dirt(6))
        );
        // Swap different items.
        assert_eq!(click_slot(&items, dirt(1), sand, false), (sand, dirt(1)));
    }
}
