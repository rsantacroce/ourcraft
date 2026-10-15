//! Placeholder until Milestone 9.
use ourcraft_mod_api::guest::Mod;
use ourcraft_mod_api::{Event, EventReply};

#[derive(Default)]
struct TeamShooter;

impl Mod for TeamShooter {
    fn on_event(&mut self, _event: Event) -> EventReply {
        EventReply::Ok
    }
}

ourcraft_mod_api::export_mod!(TeamShooter);
