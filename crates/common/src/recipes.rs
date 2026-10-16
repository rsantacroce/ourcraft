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
        shaped(&["P", "P"], &[('P', "#planks")], "core:stick", 4),
        shaped(&["PP", "PP"], &[('P', "#planks")], "core:crafting_table", 1),
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
            &["X X", " X "],
            &[('X', "core:iron_ingot")],
            "core:bucket",
            1,
        ),
        shaped(
            &["GGG", "GAG", "GGG"],
            &[('G', "core:gold_ingot"), ('A', "core:apple")],
            "core:golden_apple",
            1,
        ),
        shaped(
            &["P#P", "P#P"],
            &[('P', "core:planks"), ('#', "core:stick")],
            "core:fence",
            3,
        ),
        shaped(&["PP", "PP", "PP"], &[('P', "core:planks")], "core:door", 3),
        shaped(&["PPP", "PPP"], &[('P', "core:planks")], "core:trapdoor", 2),
        shaped(
            &["#P#", "#P#"],
            &[('#', "core:stick"), ('P', "core:planks")],
            "core:fence_gate",
            1,
        ),
        shaped(
            &["# #", "###", "# #"],
            &[('#', "core:stick")],
            "core:ladder",
            3,
        ),
        shaped(
            &["PPP", "PPP", " # "],
            &[('P', "#planks"), ('#', "core:stick")],
            "core:sign",
            3,
        ),
        shaped(&["PPP", "P P", "PPP"], &[('P', "#planks")], "core:chest", 1),
        shaped(
            &["C", "#"],
            &[('C', "core:coal"), ('#', "core:stick")],
            "core:torch",
            4,
        ),
    ];
    for (material, key) in [
        ("#planks", "wood"),
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
        r.push(shaped(
            &["XX", " #", " #"],
            &k,
            &format!("core:{key}_hoe"),
            1,
        ));
    }
    // ---- Milestone 24: our own magic ----
    r.extend([
        shaped(&["G G", " G "], &[('G', "core:glass")], "core:bottle", 3),
        shaped(
            &[" D ", "DGD", " D "],
            &[('D', "core:spark_dust"), ('G', "core:gold_ingot")],
            "core:wish_star",
            1,
        ),
        shaped(
            &[" B ", "DWD", "OOO"],
            &[
                ('B', "core:book"),
                ('D', "core:diamond"),
                ('W', "core:wish_star"),
                ('O', "core:obsidian"),
            ],
            "core:wish_table",
            1,
        ),
        shapeless(
            &["core:bottle", "core:feather", "core:spark_dust"],
            "core:hop_potion",
            1,
        ),
        shapeless(
            &["core:bottle", "core:carrot", "core:spark_dust"],
            "core:glow_potion",
            1,
        ),
        shapeless(
            &["core:bottle", "core:fish", "core:spark_dust"],
            "core:swim_potion",
            1,
        ),
    ]);
    // ---- Milestone 23: a bee hive ----
    r.push(shaped(
        &["PPP", "H H", "PPP"],
        &[('P', "#planks"), ('H', "core:honey_bowl")],
        "core:beehive",
        1,
    ));
    // ---- Milestone 22: circuits and transport ----
    r.extend([
        shaped(
            &["DDD", "DDD", "DDD"],
            &[('D', "core:spark_dust")],
            "core:spark_block",
            1,
        ),
        shapeless(&["core:spark_block"], "core:spark_dust", 9),
        shaped(
            &["#", "C"],
            &[('#', "core:stick"), ('C', "core:cobblestone")],
            "core:lever",
            1,
        ),
        shapeless(&["core:stone"], "core:button", 1),
        shaped(&["SS"], &[('S', "core:stone")], "core:pressure_plate", 1),
        shaped(
            &[" D ", "DLD", " D "],
            &[('D', "core:spark_dust"), ('L', "core:lamp")],
            "core:spark_lamp",
            1,
        ),
        shaped(
            &["DDD", "SSS"],
            &[('D', "core:spark_dust"), ('S', "core:stone")],
            "core:repeater",
            1,
        ),
        shaped(
            &["PPP", "CIC", "CDC"],
            &[
                ('P', "#planks"),
                ('C', "core:cobblestone"),
                ('I', "core:iron_ingot"),
                ('D', "core:spark_dust"),
            ],
            "core:piston",
            1,
        ),
        shapeless(&["core:piston", "core:clay_ball"], "core:sticky_piston", 1),
        shaped(
            &["PPP", "PDP", "PPP"],
            &[('P', "#planks"), ('D', "core:spark_dust")],
            "core:note_block",
            1,
        ),
        shaped(
            &["I I", "I#I", "I I"],
            &[('I', "core:iron_ingot"), ('#', "core:stick")],
            "core:rail",
            16,
        ),
        shaped(
            &["G G", "G#G", "GDG"],
            &[
                ('G', "core:gold_ingot"),
                ('#', "core:stick"),
                ('D', "core:spark_dust"),
            ],
            "core:powered_rail",
            6,
        ),
        shaped(
            &["I I", "III"],
            &[('I', "core:iron_ingot")],
            "core:minecart",
            1,
        ),
    ]);
    // ---- Milestone 21: decoration and storage ----
    r.extend([
        shaped(
            &["B B", " B "],
            &[('B', "core:brick")],
            "core:flower_pot",
            1,
        ),
        shaped(
            &["###", "#P#", "###"],
            &[('#', "core:stick"), ('P', "core:paper")],
            "core:item_frame",
            1,
        ),
        shaped(
            &["I", "T", "I"],
            &[('I', "core:iron_ingot"), ('T', "core:torch")],
            "core:lantern",
            1,
        ),
        shaped(
            &["S", "C"],
            &[('S', "core:string"), ('C', "core:clay_ball")],
            "core:candle",
            1,
        ),
        shaped(
            &["I", "I", "I"],
            &[('I', "core:iron_ingot")],
            "core:chain",
            3,
        ),
        shaped(
            &["GGG", "GGG"],
            &[('G', "core:glass")],
            "core:glass_pane",
            16,
        ),
        shaped(
            &["III", "III"],
            &[('I', "core:iron_ingot")],
            "core:iron_bars",
            16,
        ),
        shapeless(
            &["core:paper", "core:paper", "core:paper", "core:string"],
            "core:book",
            1,
        ),
        shaped(
            &["PPP", "BBB", "PPP"],
            &[('P', "#planks"), ('B', "core:book")],
            "core:bookshelf",
            1,
        ),
        shaped(
            &["PPP", " B ", " P "],
            &[('P', "#planks"), ('B', "core:book")],
            "core:lectern",
            1,
        ),
        shaped(
            &["PSP", "P P", "PSP"],
            &[('P', "#planks"), ('S', "core:planks_slab")],
            "core:barrel",
            1,
        ),
        shaped(
            &["S#S", "S S", "S S"],
            &[('S', "core:stick"), ('#', "core:string")],
            "core:scaffolding",
            6,
        ),
    ]);
    for (key, _, wool) in crate::decoration::PAINTINGS {
        let wool = crate::colours::wool_name(wool);
        r.push(shaped(
            &["###", "#W#", "###"],
            &[('#', "core:stick"), ('W', &wool)],
            &format!("core:painting_{key}"),
            1,
        ));
    }
    for (c, _, _) in crate::colours::COLOURS {
        r.push(shaped(
            &["WWW", "WWW", " # "],
            &[('W', &crate::colours::wool_name(c)), ('#', "core:stick")],
            &format!("core:{c}_banner"),
            1,
        ));
        r.push(shaped(
            &["GGG", "GGG"],
            &[('G', &format!("core:{c}_stained_glass"))],
            &format!("core:{c}_stained_glass_pane"),
            16,
        ));
    }
    // ---- Milestone 20: tools and adventure ----
    r.extend([
        shaped(
            &["  #", " #S", "# S"],
            &[('#', "core:stick"), ('S', "core:string")],
            "core:fishing_rod",
            1,
        ),
        shaped(
            &[" #S", "# S", " #S"],
            &[('#', "core:stick"), ('S', "core:string")],
            "core:bow",
            1,
        ),
        shaped(
            &["F", "#", "E"],
            &[
                ('F', "core:flint"),
                ('#', "core:stick"),
                ('E', "core:feather"),
            ],
            "core:arrow",
            4,
        ),
        shapeless(&["#wool"], "core:string", 4),
        shaped(
            &[" R ", "RWR", " R "],
            &[('R', "core:red_wool"), ('W', "core:wool")],
            "core:target",
            1,
        ),
        shaped(
            &[" I ", "ICI", " I "],
            &[('I', "core:iron_ingot"), ('C', "core:coal")],
            "core:compass",
            1,
        ),
        shaped(
            &[" G ", "GCG", " G "],
            &[('G', "core:gold_ingot"), ('C', "core:coal")],
            "core:clock",
            1,
        ),
        shaped(
            &["PPP", "PCP", "PPP"],
            &[('P', "core:paper"), ('C', "core:compass")],
            "core:map",
            1,
        ),
        shaped(
            &["G", "I", "I"],
            &[('G', "core:glass"), ('I', "core:iron_ingot")],
            "core:spyglass",
            1,
        ),
        shaped(
            &["SS ", "SS ", "  S"],
            &[('S', "core:string")],
            "core:rope",
            2,
        ),
        shaped(
            &["WWW", "S S", "I I"],
            &[
                ('W', "#wool"),
                ('S', "core:string"),
                ('I', "core:iron_ingot"),
            ],
            "core:saddle",
            1,
        ),
        shaped(
            &[" # ", "#C#", "LLL"],
            &[('#', "core:stick"), ('C', "core:coal"), ('L', "#logs")],
            "core:campfire",
            1,
        ),
    ]);
    for (key, _, material, _, _) in crate::adventure::ARMOUR_MATERIALS {
        let k = [('X', material)];
        for (pattern, piece) in [
            (&["XXX", "X X"][..], "helmet"),
            (&["X X", "XXX", "XXX"][..], "chestplate"),
            (&["XXX", "X X", "X X"][..], "leggings"),
            (&["X X", "X X"][..], "boots"),
        ] {
            r.push(shaped(pattern, &k, &format!("core:{key}_{piece}"), 1));
        }
    }
    // ---- Milestone 19: stone kinds, ice and snow ----
    let two_by_two = |from: &str, to: &str| shaped(&["XX", "XX"], &[('X', from)], to, 4);
    r.extend([
        two_by_two("core:stone", "core:stone_bricks"),
        two_by_two("core:sandstone", "core:cut_sandstone"),
        two_by_two("core:granite", "core:polished_granite"),
        two_by_two("core:diorite", "core:polished_diorite"),
        two_by_two("core:andesite", "core:polished_andesite"),
        two_by_two("core:cobbled_deep_slate", "core:polished_deep_slate"),
        two_by_two("core:polished_deep_slate", "core:deep_slate_bricks"),
        shaped(&["XX", "XX"], &[('X', "core:sand")], "core:sandstone", 1),
        shapeless(
            &["core:stone_bricks", "#leaves"],
            "core:mossy_stone_bricks",
            1,
        ),
        shapeless(
            &["core:cobblestone", "#leaves"],
            "core:mossy_cobblestone",
            1,
        ),
        shaped(
            &["III", "III", "III"],
            &[('I', "core:ice")],
            "core:packed_ice",
            1,
        ),
        shaped(
            &["SS", "SS"],
            &[('S', "core:snowball")],
            "core:snow_block",
            1,
        ),
        shaped(&["SSS"], &[('S', "core:snow_block")], "core:snow_layer", 6),
    ]);
    for (block, key) in crate::stones::WALLS {
        r.push(shaped(
            &["XXX", "XXX"],
            &[('X', block)],
            &format!("core:{key}_wall"),
            6,
        ));
    }
    for (key, _, _) in crate::stones::STAIRS {
        let block = format!("core:{key}");
        let k = [('X', block.as_str())];
        r.push(shaped(
            &["X  ", "XX ", "XXX"],
            &k,
            &format!("core:{key}_stairs"),
            4,
        ));
        r.push(shaped(&["XXX"], &k, &format!("core:{key}_slab"), 6));
    }
    // ---- Milestone 19: woods ----
    for wood in crate::woods::Wood::ALL {
        let planks = wood.planks();
        let p = planks.as_str();
        if wood != crate::woods::Wood::Oak {
            r.push(shapeless(&[&wood.log()], p, 4));
            let k = [('X', p)];
            r.push(shaped(&["X  ", "XX ", "XXX"], &k, &wood.stairs(), 4));
            r.push(shaped(&["XXX"], &k, &wood.slab(), 6));
            r.push(shaped(
                &["P#P", "P#P"],
                &[('P', p), ('#', "core:stick")],
                &wood.fence(),
                3,
            ));
            r.push(shaped(
                &["#P#", "#P#"],
                &[('#', "core:stick"), ('P', p)],
                &wood.fence_gate(),
                1,
            ));
            r.push(shaped(&["PP", "PP", "PP"], &[('P', p)], &wood.door(), 3));
            r.push(shaped(&["PPP", "PPP"], &[('P', p)], &wood.trapdoor(), 2));
        }
        r.push(shaped(&["P P", "PPP"], &[('P', p)], &wood.boat(), 1));
    }
    // ---- Milestone 18: dyes and dyed things ----
    use crate::colours::{COLOURS, FLOWERS, bed_name, wool_name};
    for (flower, _, dye) in FLOWERS {
        r.push(shapeless(&[flower], &format!("core:{dye}_dye"), 1));
    }
    let dye = |c: &str| format!("core:{c}_dye");
    for (from, to, n) in [
        (&["core:bone_meal"][..], "white", 1),
        (&["core:coal"], "black", 1),
        (&["core:brown_mushroom"], "brown", 1),
        (&["core:beetroot"], "red", 1),
    ] {
        r.push(shapeless(from, &dye(to), n));
    }
    for (a, b, to) in [
        ("white", "black", "gray"),
        ("white", "gray", "light_gray"),
        ("white", "red", "pink"),
        ("white", "blue", "light_blue"),
        ("white", "green", "lime"),
        ("blue", "green", "cyan"),
        ("red", "blue", "purple"),
        ("purple", "pink", "magenta"),
        ("red", "yellow", "orange"),
    ] {
        r.push(shapeless(&[&dye(a), &dye(b)], &dye(to), 2));
    }
    for (c, _, _) in COLOURS {
        let d = dye(c);
        let wool = wool_name(c);
        if c != "white" {
            r.push(shapeless(&["core:wool", &d], &wool, 1));
        }
        r.push(shaped(
            &["WWW", "PPP"],
            &[('W', &wool), ('P', "#planks")],
            &bed_name(c),
            1,
        ));
        r.push(shaped(
            &["WW"],
            &[('W', &wool)],
            &format!("core:{c}_carpet"),
            3,
        ));
        r.push(shaped(
            &["GGG", "GDG", "GGG"],
            &[('G', "core:glass"), ('D', &d)],
            &format!("core:{c}_stained_glass"),
            8,
        ));
        r.push(shaped(
            &["CCC", "CDC", "CCC"],
            &[('C', "core:clay"), ('D', &d)],
            &format!("core:{c}_clay_tiles"),
            8,
        ));
    }
    // ---- Milestone 17: farming and food ----
    r.extend([
        shaped(&["WWW"], &[('W', "core:wheat")], "core:bread", 1),
        shaped(
            &["WSW"],
            &[('W', "core:wheat"), ('S', "core:sugar")],
            "core:cookie",
            8,
        ),
        shaped(
            &["MMM", "SES", "WWW"],
            &[
                ('M', "core:milk_bucket"),
                ('S', "core:sugar"),
                ('E', "core:egg"),
                ('W', "core:wheat"),
            ],
            "core:cake",
            1,
        ),
        shapeless(
            &["core:pumpkin", "core:sugar", "core:egg"],
            "core:pumpkin_pie",
            1,
        ),
        shapeless(&["core:pumpkin"], "core:pumpkin_seeds", 4),
        shapeless(&["core:melon_slice"], "core:melon_seeds", 1),
        shaped(
            &["MMM", "MMM", "MMM"],
            &[('M', "core:melon_slice")],
            "core:melon",
            1,
        ),
        shapeless(&["core:sugar_cane"], "core:sugar", 1),
        shaped(&["CCC"], &[('C', "core:sugar_cane")], "core:paper", 3),
        shaped(&["P P", " P "], &[('P', "#planks")], "core:bowl", 4),
        shapeless(
            &[
                "core:bowl",
                "core:beetroot",
                "core:beetroot",
                "core:beetroot",
                "core:beetroot",
                "core:beetroot",
                "core:beetroot",
            ],
            "core:beetroot_soup",
            1,
        ),
        shapeless(
            &["core:bowl", "core:red_mushroom", "core:brown_mushroom"],
            "core:mushroom_stew",
            1,
        ),
        shaped(
            &["P", "T"],
            &[('P', "core:pumpkin"), ('T', "core:torch")],
            "core:pumpkin_lantern",
            1,
        ),
        shaped(
            &["P P", "P P", "PPP"],
            &[('P', "#planks")],
            "core:composter",
            1,
        ),
    ]);
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
        /// Each cell: empty, or any of these items.
        cells: Vec<Option<Vec<ItemId>>>,
    },
    /// Each ingredient is any of its items.
    Shapeless(Vec<Vec<ItemId>>),
}

/// What a name in a recipe stands for: one item, or a group such as
/// `#planks` (planks of any wood), `#logs`, `#wool` or `#leaves`.
fn ingredient(name: &str, items: &ItemRegistry) -> Option<Vec<ItemId>> {
    let Some(tag) = name.strip_prefix('#') else {
        return items.by_name(name).map(|i| vec![i]);
    };
    let (plain, suffix) = match tag {
        "planks" => ("core:planks", "_planks"),
        "logs" => ("core:log", "_log"),
        "wool" => ("core:wool", "_wool"),
        "leaves" => ("core:leaves", "_leaves"),
        _ => return None,
    };
    let members: Vec<ItemId> = items
        .iter()
        .filter(|(_, d)| {
            d.name == plain || (d.name.starts_with("core:") && d.name.ends_with(suffix))
        })
        .map(|(id, _)| id)
        .collect();
    (!members.is_empty()).then_some(members)
}

/// Can every present item be matched to a different ingredient?
fn shapeless_match(ingredients: &[Vec<ItemId>], present: &[ItemId]) -> bool {
    fn assign(ingredients: &[Vec<ItemId>], present: &[ItemId], used: &mut [bool]) -> bool {
        let Some((first, rest)) = present.split_first() else {
            return true;
        };
        for (i, ing) in ingredients.iter().enumerate() {
            if !used[i] && ing.contains(first) {
                used[i] = true;
                if assign(ingredients, rest, used) {
                    return true;
                }
                used[i] = false;
            }
        }
        false
    }
    ingredients.len() == present.len()
        && assign(ingredients, present, &mut vec![false; ingredients.len()])
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
                                .and_then(|(_, n)| ingredient(n, items));
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
                    let ids: Option<Vec<Vec<ItemId>>> =
                        ingredients.iter().map(|n| ingredient(n, items)).collect();
                    let (Some(ids), Some(res)) = (ids, items.by_name(result)) else {
                        continue;
                    };
                    if ids.is_empty() || ids.len() > 9 {
                        continue;
                    }
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
        let present: Vec<ItemId> = grid.iter().flatten().copied().collect();
        let fits = |want: &Option<Vec<ItemId>>, have: Option<ItemId>| match (want, have) {
            (None, None) => true,
            (Some(set), Some(item)) => set.contains(&item),
            _ => false,
        };

        for (recipe, result, count) in &self.recipes {
            let matches = match recipe {
                Resolved::Shaped {
                    w: rw,
                    h: rh,
                    cells,
                } => {
                    *rw == w
                        && *rh == h
                        && ((0..h).all(|y| (0..w).all(|x| fits(&cells[y * w + x], cell(x, y))))
                            || (0..h).all(|y| {
                                (0..w).all(|x| fits(&cells[y * w + (w - 1 - x)], cell(x, y)))
                            }))
                }
                Resolved::Shapeless(ids) => shapeless_match(ids, &present),
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
    fn any_planks_will_do() {
        let (items, book) = book();
        let oak = items.by_name("core:planks");
        let birch = items.by_name("core:birch_planks");
        let pine = items.by_name("core:pine_planks");
        // A crafting table from mixed planks.
        let grid = [oak, birch, pine, oak];
        assert_eq!(
            book.find(&grid, 2).map(|r| r.0),
            items.by_name("core:crafting_table")
        );
        // Sticks from birch; but a birch fence needs birch planks.
        assert_eq!(
            book.find(&[birch, None, birch, None], 2).map(|r| r.0),
            items.by_name("core:stick")
        );
        let stick = items.by_name("core:stick");
        let fence = [birch, stick, birch, birch, stick, birch, None, None, None];
        assert_eq!(
            book.find(&fence, 3).map(|r| r.0),
            items.by_name("core:birch_fence")
        );
        // Birch logs make birch planks.
        assert_eq!(
            book.find(&[items.by_name("core:birch_log"), None, None, None], 2),
            birch.map(|b| (b, 4))
        );
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
