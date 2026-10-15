//! Crafting recipes.
//!
//! A recipe is either **shaped** (a little picture: three planks in a row
//! over two sticks makes a pickaxe) or **shapeless** (any arrangement: a log
//! anywhere makes planks). Shaped recipes may sit anywhere in the grid, and
//! also work mirrored left-to-right.
//!
//! Recipes use item *names*, so they can be sent to clients and written by
//! mods; [`RecipeBook::new`] turns them into item numbers once.

use serde::{Deserialize, Serialize};

use crate::item::{ItemId, ItemRegistry};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recipe {
    Shaped {
        /// Rows of the picture, one character per cell; `' '` is empty.
        pattern: Vec<String>,
        /// What each character means: (character, item name).
        key: Vec<(char, String)>,
        result: String,
        count: u16,
    },
    Shapeless {
        ingredients: Vec<String>,
        result: String,
        count: u16,
    },
}

impl From<ourcraft_mod_api::RecipeSpec> for Recipe {
    fn from(r: ourcraft_mod_api::RecipeSpec) -> Self {
        use ourcraft_mod_api::RecipeSpec as R;
        match r {
            R::Shaped {
                pattern,
                key,
                result,
                count,
            } => Recipe::Shaped {
                pattern,
                key,
                result,
                count,
            },
            R::Shapeless {
                ingredients,
                result,
                count,
            } => Recipe::Shapeless {
                ingredients,
                result,
                count,
            },
        }
    }
}

fn shaped(pattern: &[&str], key: &[(char, &str)], result: &str, count: u16) -> Recipe {
    Recipe::Shaped {
        pattern: pattern.iter().map(|r| r.to_string()).collect(),
        key: key.iter().map(|(c, n)| (*c, n.to_string())).collect(),
        result: result.into(),
        count,
    }
}

fn shapeless(ingredients: &[&str], result: &str, count: u16) -> Recipe {
    Recipe::Shapeless {
        ingredients: ingredients.iter().map(|s| s.to_string()).collect(),
        result: result.into(),
        count,
    }
}

/// The recipes that come with the game.
pub fn builtin_recipes() -> Vec<Recipe> {
    let mut r = vec![
        shapeless(&["core:log"], "core:planks", 4),
        shaped(&["P", "P"], &[('P', "core:planks")], "core:stick", 4),
        shaped(
            &["PP", "PP"],
            &[('P', "core:planks")],
            "core:crafting_table",
            1,
        ),
        shaped(
            &["CCC", "C C", "CCC"],
            &[('C', "core:cobblestone")],
            "core:furnace",
            1,
        ),
        shaped(&["BB", "BB"], &[('B', "core:brick")], "core:bricks", 1),
        shaped(&["CC", "CC"], &[('C', "core:clay_ball")], "core:clay", 1),
        shaped(
            &["GGG", "GCG", "GGG"],
            &[('G', "core:glass"), ('C', "core:coal")],
            "core:lamp",
            2,
        ),
        shaped(&[" X", "X "], &[('X', "core:iron_ingot")], "core:shears", 1),
        shaped(
            &["P#P", "P#P"],
            &[('P', "core:planks"), ('#', "core:stick")],
            "core:fence",
            3,
        ),
        shaped(&["PP", "PP", "PP"], &[('P', "core:planks")], "core:door", 3),
        shaped(&["PPP", "PPP"], &[('P', "core:planks")], "core:trapdoor", 2),
        shaped(
            &["# #", "###", "# #"],
            &[('#', "core:stick")],
            "core:ladder",
            3,
        ),
        shaped(
            &["PPP", "PPP", " # "],
            &[('P', "core:planks"), ('#', "core:stick")],
            "core:sign",
            3,
        ),
        shaped(
            &["WWW", "PPP"],
            &[('W', "core:wool"), ('P', "core:planks")],
            "core:bed",
            1,
        ),
        shaped(
            &["PPP", "P P", "PPP"],
            &[('P', "core:planks")],
            "core:chest",
            1,
        ),
        shaped(
            &["C", "#"],
            &[('C', "core:coal"), ('#', "core:stick")],
            "core:torch",
            4,
        ),
    ];
    for (material, key) in [
        ("core:planks", "wood"),
        ("core:cobblestone", "stone"),
        ("core:iron_ingot", "iron"),
        ("core:diamond", "diamond"),
    ] {
        let k = [('X', material), ('#', "core:stick")];
        r.push(shaped(
            &["XXX", " # ", " # "],
            &k,
            &format!("core:{key}_pickaxe"),
            1,
        ));
        r.push(shaped(
            &["XX", "X#", " #"],
            &k,
            &format!("core:{key}_axe"),
            1,
        ));
        r.push(shaped(
            &["X", "#", "#"],
            &k,
            &format!("core:{key}_shovel"),
            1,
        ));
    }
    for mat in ["planks", "cobblestone", "stone", "bricks"] {
        let block = format!("core:{mat}");
        let k = [('X', block.as_str())];
        r.push(shaped(
            &["X  ", "XX ", "XXX"],
            &k,
            &format!("core:{mat}_stairs"),
            4,
        ));
        r.push(shaped(&["XXX"], &k, &format!("core:{mat}_slab"), 6));
    }
    for (ingot, block) in [
        ("core:iron_ingot", "core:iron_block"),
        ("core:gold_ingot", "core:gold_block"),
        ("core:diamond", "core:diamond_block"),
    ] {
        r.push(shaped(&["XXX", "XXX", "XXX"], &[('X', ingot)], block, 1));
        r.push(shapeless(&[block], ingot, 9));
    }
    r
}

/// A recipe with item numbers instead of names.
#[derive(Clone, Debug, PartialEq)]
enum Resolved {
    Shaped {
        w: usize,
        h: usize,
        cells: Vec<Option<ItemId>>,
    },
    Shapeless(Vec<ItemId>),
}

/// All recipes, ready to match against a crafting grid.
#[derive(Clone, Debug, Default)]
pub struct RecipeBook {
    recipes: Vec<(Resolved, ItemId, u16)>,
}

impl RecipeBook {
    /// Resolve names to items. Recipes mentioning unknown items are skipped.
    pub fn new(recipes: &[Recipe], items: &ItemRegistry) -> Self {
        let mut out = Vec::new();
        for r in recipes {
            let resolved = match r {
                Recipe::Shaped {
                    pattern,
                    key,
                    result,
                    count,
                } => {
                    let h = pattern.len();
                    let w = pattern.iter().map(|p| p.chars().count()).max().unwrap_or(0);
                    if w == 0 || w > 3 || h == 0 || h > 3 {
                        continue;
                    }
                    let mut cells = Vec::with_capacity(w * h);
                    let mut ok = true;
                    for row in pattern {
                        let mut chars = row.chars();
                        for _ in 0..w {
                            let c = chars.next().unwrap_or(' ');
                            if c == ' ' {
                                cells.push(None);
                                continue;
                            }
                            let item = key
                                .iter()
                                .find(|(k, _)| *k == c)
                                .and_then(|(_, n)| items.by_name(n));
                            ok &= item.is_some();
                            cells.push(item);
                        }
                    }
                    let Some(res) = items.by_name(result).filter(|_| ok) else {
                        continue;
                    };
                    (Resolved::Shaped { w, h, cells }, res, *count)
                }
                Recipe::Shapeless {
                    ingredients,
                    result,
                    count,
                } => {
                    let ids: Option<Vec<ItemId>> =
                        ingredients.iter().map(|n| items.by_name(n)).collect();
                    let (Some(mut ids), Some(res)) = (ids, items.by_name(result)) else {
                        continue;
                    };
                    if ids.is_empty() || ids.len() > 9 {
                        continue;
                    }
                    ids.sort();
                    (Resolved::Shapeless(ids), res, *count)
                }
            };
            out.push(resolved);
        }
        Self { recipes: out }
    }

    /// What a crafting grid makes. `grid` is `size × size` cells, row by row
    /// (size 2 in the inventory, 3 on a crafting table).
    pub fn find(&self, grid: &[Option<ItemId>], size: usize) -> Option<(ItemId, u16)> {
        // Trim the grid to the cells actually used.
        let used = |x: usize, y: usize| grid[y * size + x].is_some();
        let cols: Vec<usize> = (0..size)
            .filter(|&x| (0..size).any(|y| used(x, y)))
            .collect();
        let rows: Vec<usize> = (0..size)
            .filter(|&y| (0..size).any(|x| used(x, y)))
            .collect();
        let (Some(&x0), Some(&x1), Some(&y0), Some(&y1)) =
            (cols.first(), cols.last(), rows.first(), rows.last())
        else {
            return None;
        };
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let cell = |x: usize, y: usize| grid[(y0 + y) * size + x0 + x];
        let mut present: Vec<ItemId> = grid.iter().flatten().copied().collect();
        present.sort();

        for (recipe, result, count) in &self.recipes {
            let matches = match recipe {
                Resolved::Shaped {
                    w: rw,
                    h: rh,
                    cells,
                } => {
                    *rw == w
                        && *rh == h
                        && ((0..h).all(|y| (0..w).all(|x| cells[y * w + x] == cell(x, y)))
                            || (0..h)
                                .all(|y| (0..w).all(|x| cells[y * w + (w - 1 - x)] == cell(x, y))))
                }
                Resolved::Shapeless(ids) => *ids == present,
            };
            if matches {
                return Some((*result, *count));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockRegistry, blocks};

    fn book() -> (ItemRegistry, RecipeBook) {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let book = RecipeBook::new(&builtin_recipes(), &items);
        (items, book)
    }

    #[test]
    fn every_builtin_recipe_resolves() {
        let (items, book) = book();
        assert_eq!(book.recipes.len(), builtin_recipes().len());
        assert!(items.by_name("core:diamond_shovel").is_some());
    }

    #[test]
    fn log_anywhere_makes_planks() {
        let (_, book) = book();
        let log = Some(ItemId::from_block(blocks::LOG));
        let mut grid = [None; 4];
        grid[3] = log;
        assert_eq!(
            book.find(&grid, 2),
            Some((ItemId::from_block(blocks::PLANKS), 4))
        );
        let mut grid = [None; 9];
        grid[4] = log;
        assert!(book.find(&grid, 3).is_some());
        assert_eq!(book.find(&[None; 4], 2), None);
    }

    #[test]
    fn shaped_recipes_need_their_shape() {
        let (items, book) = book();
        let p = Some(ItemId::from_block(blocks::PLANKS));
        let s = items.by_name("core:stick");
        // Pickaxe on a crafting table.
        let grid = [p, p, p, None, s, None, None, s, None];
        assert_eq!(
            book.find(&grid, 3),
            Some((items.by_name("core:wood_pickaxe").unwrap(), 1))
        );
        // Planks in a column, anywhere, make sticks.
        let grid = [None, p, None, p];
        assert_eq!(book.find(&grid, 2), Some((s.unwrap(), 4)));
        // Planks side by side make nothing.
        assert_eq!(book.find(&[p, p, None, None], 2), None);
    }

    #[test]
    fn mirrored_axe_works() {
        let (items, book) = book();
        let c = Some(ItemId::from_block(blocks::COBBLESTONE));
        let s = items.by_name("core:stick");
        let axe = items.by_name("core:stone_axe").unwrap();
        let left = [c, c, None, c, s, None, None, s, None];
        let right = [None, c, c, None, s, c, None, s, None];
        assert_eq!(book.find(&left, 3), Some((axe, 1)));
        assert_eq!(book.find(&right, 3), Some((axe, 1)));
    }
}
