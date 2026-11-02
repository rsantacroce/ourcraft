//! Achievements: gentle goals for survival players (see
//! [`ourcraft_common::achievements`] for the list).
//!
//! A couple of times a second each survival player's inventory and state
//! are checked; other modules report the rest with [`DeedDone`]. A new
//! achievement gets a big title, a cheer and a chat line suggesting what
//! to try next. `/goals` lists them all.

use bevy::prelude::*;
use ourcraft_common::achievements::{self, Achievement, Deed};
use ourcraft_common::dimension::Dimension;
use ourcraft_common::protocol::{Channel, ClientMessage, ServerMessage};
use renet::ClientId;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::OpenWorld;

/// Seconds between inventory checks.
const CHECK_EVERY: f32 = 0.5;

pub struct AchievementsPlugin;

impl Plugin for AchievementsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DeedDone>().add_systems(
            Update,
            (goals_command, check_achievements).after(crate::players::PlayersSet),
        );
    }
}

/// Someone did something that may earn an achievement.
#[derive(Message, Clone, Copy, Debug)]
pub struct DeedDone {
    pub client: ClientId,
    pub deed: Deed,
}

/// Is this chat line `/goals`?
pub fn is_command(text: &str) -> bool {
    matches!(
        text.split_whitespace().next(),
        Some("/goals" | "/achievements")
    )
}

fn check_achievements(
    time: Res<Time>,
    mut since: Local<f32>,
    mut deeds: MessageReader<DeedDone>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    mut open: ResMut<OpenWorld>,
    mut net: ResMut<Network>,
) {
    let reported: Vec<DeedDone> = deeds.read().copied().collect();
    *since += time.delta_secs();
    if *since < CHECK_EVERY && reported.is_empty() {
        return;
    }
    *since = 0.0;
    for (client, entity) in &players_map.0 {
        let Ok(p) = players.get(*entity) else {
            continue;
        };
        let Some(name) = p.name.as_ref() else {
            continue;
        };
        if p.creative {
            continue;
        }
        let done = open.achievements.get(name).cloned().unwrap_or_default();
        let has = |id: &str| done.contains(id);
        let names: Vec<&str> = p
            .inventory
            .slots
            .iter()
            .chain(&p.inventory.armour)
            .flatten()
            .map(|s| open.items.name(s.item))
            .collect();
        let mut new = achievements::earned_by_items(names.iter().copied(), has);
        let mut deeds: Vec<Deed> = reported
            .iter()
            .filter(|d| d.client == *client)
            .map(|d| d.deed)
            .collect();
        if p.sleeping {
            deeds.push(Deed::Sleep);
        }
        if p.riding.is_some() {
            deeds.push(Deed::Ride);
        }
        if p.dimension == Dimension::CloudIsles {
            deeds.push(Deed::VisitCloudIsles);
        }
        let at = p.position();
        if p.dimension == Dimension::Overworld
            && open
                .generator
                .village_near(at.x.floor() as i32, at.z.floor() as i32)
                .is_some()
        {
            deeds.push(Deed::FindVillage);
        }
        for d in deeds {
            if let Some(a) = achievements::for_deed(d)
                && !has(a.id)
                && !new.iter().any(|n| n.id == a.id)
            {
                new.push(a);
            }
        }
        if new.is_empty() {
            continue;
        }
        let name = name.clone();
        let entry = open.achievements.entry(name).or_default();
        for a in &new {
            entry.insert(a.id.to_owned());
        }
        let done = entry.clone();
        announce(&mut net, *client, &new, |id| done.contains(id));
    }
}

/// Tell a player what they've just earned, and what to try next.
fn announce(
    net: &mut Network,
    client: ClientId,
    new: &[&'static Achievement],
    done: impl Fn(&str) -> bool,
) {
    let titles: Vec<&str> = new.iter().map(|a| a.title).collect();
    net.send(
        client,
        Channel::Reliable,
        &ServerMessage::Title {
            text: match titles.as_slice() {
                [one] => format!("Achievement! {one}"),
                [first, rest @ ..] => format!("Achievement! {first} and {} more", rest.len()),
                [] => return,
            },
            seconds: 3.0,
        },
    );
    net.send(
        client,
        Channel::Reliable,
        &ServerMessage::Sound {
            name: "cheer".into(),
            at: None,
        },
    );
    let count = achievements::ALL.iter().filter(|a| done(a.id)).count();
    let next = match achievements::next(&done) {
        Some(a) => format!(" Next: {} ({}).", a.title, a.hint),
        None => " That's all of them. Well done!".into(),
    };
    net.send(
        client,
        Channel::Reliable,
        &ServerMessage::Chat {
            from: None,
            text: format!(
                "You did it: {}! ({count} of {} done.){next}",
                titles.join(", "),
                achievements::ALL.len()
            ),
        },
    );
}

/// `/goals`: what's done and what's still to do.
fn goals_command(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::Chat { text } = msg else {
            continue;
        };
        if !is_command(text.trim()) {
            continue;
        }
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        let Some(name) = p.name.as_ref() else {
            continue;
        };
        let done = open.achievements.get(name);
        let has = |id: &str| done.is_some_and(|d| d.contains(id));
        for text in goals_text(&has, p.creative) {
            net.send(
                *client,
                Channel::Reliable,
                &ServerMessage::Chat { from: None, text },
            );
        }
    }
}

/// The `/goals` reply, a few chat lines long.
fn goals_text(has: &impl Fn(&str) -> bool, creative: bool) -> Vec<String> {
    let (done, todo): (Vec<&Achievement>, Vec<&Achievement>) =
        achievements::ALL.iter().partition(|a| has(a.id));
    let titles = |l: &[&Achievement]| l.iter().map(|a| a.title).collect::<Vec<_>>().join(", ");
    let mut lines = vec![format!(
        "Goals: {} of {} done.",
        done.len(),
        achievements::ALL.len()
    )];
    if !done.is_empty() {
        lines.push(format!("Done: {}.", titles(&done)));
    }
    match todo.first() {
        Some(next) => {
            lines.push(format!("Still to do: {}.", titles(&todo)));
            lines.push(format!("Next: {} ({}).", next.title, next.hint));
        }
        None => lines.push("All done. Well done!".into()),
    }
    if creative {
        lines.push("Goals count in survival only (press G).".into());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals_list_marks_what_is_done() {
        let lines = goals_text(&|id| id == "wood", false);
        assert!(lines[0].starts_with("Goals: 1 of"));
        assert_eq!(lines[1], "Done: Get some wood.");
        assert!(lines[2].starts_with("Still to do: Make planks"));
        assert!(lines[3].starts_with("Next: Make planks"));
        assert!(
            goals_text(&|_| false, true)
                .last()
                .unwrap()
                .contains("survival only")
        );
    }
}
