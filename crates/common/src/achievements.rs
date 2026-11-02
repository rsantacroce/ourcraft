//! Achievements: gentle goals that guide new players through the game,
//! from getting some wood to visiting the Cloud Isles.
//!
//! They're in order, so there's always a "next" one to suggest. Most are
//! earned by having an item (checked against the inventory); a few by
//! doing something (see [`Goal::Do`]). They count in survival only:
//! creative hands you everything.

/// What earns an achievement.
#[derive(Clone, Copy, Debug)]
pub enum Goal {
    /// Have an item whose name passes this test.
    Have(fn(&str) -> bool),
    /// Do something the server notices (see [`Deed`]).
    Do(Deed),
}

/// Things you can do that count towards achievements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Deed {
    BopMonster,
    Sleep,
    Ride,
    VisitCloudIsles,
}

#[derive(Clone, Copy, Debug)]
pub struct Achievement {
    /// Saved by this, so never change one.
    pub id: &'static str,
    /// What to do, as a goal ("Make a crafting table").
    pub title: &'static str,
    /// How to do it.
    pub hint: &'static str,
    pub goal: Goal,
}

const fn have(
    id: &'static str,
    title: &'static str,
    hint: &'static str,
    test: fn(&str) -> bool,
) -> Achievement {
    Achievement {
        id,
        title,
        hint,
        goal: Goal::Have(test),
    }
}

const fn deed(id: &'static str, title: &'static str, hint: &'static str, d: Deed) -> Achievement {
    Achievement {
        id,
        title,
        hint,
        goal: Goal::Do(d),
    }
}

/// Every achievement, in the order they're suggested.
pub const ALL: &[Achievement] = &[
    have(
        "wood",
        "Get some wood",
        "hold the break button on a tree trunk",
        |n| n.ends_with("_log") || n == "core:log",
    ),
    have(
        "planks",
        "Make planks",
        "put a log in the crafting grid in your inventory",
        |n| n.ends_with("planks"),
    ),
    have(
        "crafting_table",
        "Make a crafting table",
        "four planks in the crafting grid",
        |n| n == "core:crafting_table",
    ),
    have(
        "wood_pickaxe",
        "Make a wooden pickaxe",
        "three planks over two sticks, on a crafting table",
        |n| n == "core:wood_pickaxe",
    ),
    have(
        "stone_pickaxe",
        "Make a stone pickaxe",
        "dig some stone, then cobblestone over two sticks",
        |n| n == "core:stone_pickaxe",
    ),
    have(
        "torch",
        "Make a torch",
        "coal (from dark-speckled stone) over a stick",
        |n| n == "core:torch",
    ),
    have(
        "sword",
        "Make a sword",
        "two planks or stones over a stick",
        |n| n.ends_with("_sword"),
    ),
    have(
        "furnace",
        "Build a furnace",
        "eight cobblestones round the edge of a crafting table",
        |n| n == "core:furnace",
    ),
    have(
        "iron",
        "Smelt some iron",
        "cook iron ore (orange-speckled stone) in a furnace",
        |n| n == "core:iron_ingot",
    ),
    deed(
        "bop",
        "Bop a monster away",
        "monsters come out at night; a few bops and they puff away",
        Deed::BopMonster,
    ),
    have("bed", "Make a bed", "three wool over three planks", |n| {
        n == "core:bed" || n.ends_with("_bed")
    }),
    deed(
        "sleep",
        "Sleep in a bed",
        "use your bed at night to skip to morning",
        Deed::Sleep,
    ),
    have(
        "bread",
        "Bake bread",
        "grow wheat from seeds, then three wheat in a row",
        |n| n == "core:bread",
    ),
    have(
        "fish",
        "Catch a fish",
        "cast a fishing rod into water and wait for a bite",
        |n| n == "core:fish",
    ),
    deed(
        "ride",
        "Ride a pig",
        "put a saddle on a pig, then hop on",
        Deed::Ride,
    ),
    have(
        "diamond",
        "Find a diamond",
        "deep underground, with an iron pickaxe",
        |n| n == "core:diamond",
    ),
    have(
        "sparkly",
        "Make something sparkly",
        "a wish star at a wish table makes tools and armour sparkle",
        |n| n.starts_with("core:sparkly_"),
    ),
    deed(
        "cloud_isles",
        "Visit the Cloud Isles",
        "light a sunstone portal with a feather",
        Deed::VisitCloudIsles,
    ),
];

pub fn by_id(id: &str) -> Option<&'static Achievement> {
    ALL.iter().find(|a| a.id == id)
}

/// The first achievement not yet earned.
pub fn next<'a>(done: impl Fn(&str) -> bool + 'a) -> Option<&'static Achievement> {
    ALL.iter().find(|a| !done(a.id))
}

/// Achievements newly earned by having these items (by name).
pub fn earned_by_items<'a>(
    names: impl Iterator<Item = &'a str> + Clone,
    done: impl Fn(&str) -> bool,
) -> Vec<&'static Achievement> {
    ALL.iter()
        .filter(|a| !done(a.id))
        .filter(|a| match a.goal {
            Goal::Have(test) => names.clone().any(test),
            Goal::Do(_) => false,
        })
        .collect()
}

/// The achievement for doing something, if there is one.
pub fn for_deed(d: Deed) -> Option<&'static Achievement> {
    ALL.iter().find(|a| matches!(a.goal, Goal::Do(x) if x == d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockRegistry;
    use crate::item::ItemRegistry;

    #[test]
    fn ids_are_unique() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(ALL[i + 1..].iter().all(|b| b.id != a.id), "{}", a.id);
        }
    }

    #[test]
    fn every_item_goal_can_be_reached() {
        let blocks = BlockRegistry::with_builtin_blocks();
        let items = ItemRegistry::build(&blocks, &[]);
        let names: Vec<&str> = items.iter().map(|(_, d)| d.name.as_str()).collect();
        for a in ALL {
            if let Goal::Have(test) = a.goal {
                assert!(names.iter().any(|n| test(n)), "nothing earns {}", a.id);
            }
        }
    }

    #[test]
    fn items_earn_and_next_moves_on() {
        let got = earned_by_items(["core:birch_log", "core:stick"].into_iter(), |_| false);
        assert_eq!(got.iter().map(|a| a.id).collect::<Vec<_>>(), ["wood"]);
        assert!(earned_by_items(["core:log"].into_iter(), |id| id == "wood").is_empty());
        assert_eq!(next(|id| id == "wood").unwrap().id, "planks");
        assert_eq!(for_deed(Deed::Sleep).unwrap().id, "sleep");
    }
}
