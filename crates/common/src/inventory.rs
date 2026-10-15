//! A simple inventory: 36 slots of block stacks. Slots 0–8 are the hotbar.

use crate::block::BlockId;

pub const HOTBAR_SLOTS: usize = 9;
pub const INVENTORY_SLOTS: usize = 36;
pub const MAX_STACK: u16 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub block: BlockId,
    pub count: u16,
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

    /// Add blocks, filling existing stacks first, then empty slots (hotbar
    /// first). Returns how many didn't fit.
    pub fn add(&mut self, block: BlockId, mut count: u16) -> u16 {
        for slot in self.slots.iter_mut().flatten() {
            if slot.block == block && slot.count < MAX_STACK {
                let n = count.min(MAX_STACK - slot.count);
                slot.count += n;
                count -= n;
                if count == 0 {
                    return 0;
                }
            }
        }
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                let n = count.min(MAX_STACK);
                *slot = Some(ItemStack { block, count: n });
                count -= n;
                if count == 0 {
                    return 0;
                }
            }
        }
        count
    }

    /// Use one block from the selected hotbar slot, returning it.
    pub fn take_selected(&mut self) -> Option<BlockId> {
        let slot = &mut self.slots[self.selected];
        let stack = slot.as_mut()?;
        let block = stack.block;
        stack.count -= 1;
        if stack.count == 0 {
            *slot = None;
        }
        Some(block)
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

    /// Total count of a block across all slots.
    pub fn count(&self, block: BlockId) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.block == block)
            .map(|s| s.count as u32)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;

    #[test]
    fn add_stacks_then_spills() {
        let mut inv = Inventory::default();
        assert_eq!(inv.add(blocks::DIRT, 100), 0);
        assert_eq!(
            inv.slots[0],
            Some(ItemStack {
                block: blocks::DIRT,
                count: 64
            })
        );
        assert_eq!(
            inv.slots[1],
            Some(ItemStack {
                block: blocks::DIRT,
                count: 36
            })
        );
        assert_eq!(inv.add(blocks::DIRT, 10), 0);
        assert_eq!(inv.slots[1].unwrap().count, 46);
        assert_eq!(inv.count(blocks::DIRT), 110);
    }

    #[test]
    fn full_inventory_returns_leftovers() {
        let mut inv = Inventory::default();
        let capacity = INVENTORY_SLOTS as u16 * MAX_STACK;
        assert_eq!(inv.add(blocks::STONE, capacity + 5), 5);
    }

    #[test]
    fn take_selected_empties_slot() {
        let mut inv = Inventory::default();
        inv.add(blocks::SAND, 2);
        assert_eq!(inv.take_selected(), Some(blocks::SAND));
        assert_eq!(inv.take_selected(), Some(blocks::SAND));
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
}
