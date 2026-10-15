//! Finding games on the local network.
//!
//! Servers that accept network players broadcast a small "I'm here" message
//! every second (see the server's `announce_on_lan`). While the Join screen
//! is open we listen for those and list what we hear.
//!
//! On iPhone/iPad, receiving broadcasts may need Apple's multicast
//! permission for the app; typing the address always works.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

use bevy::prelude::*;
use ourcraft_common::protocol::{LAN_DISCOVERY_PORT, LanBeacon, PROTOCOL_ID, decode};

pub struct LanPlugin;

impl Plugin for LanPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LanBrowser>()
            .add_systems(Update, listen);
    }
}

/// A server heard on the network.
#[derive(Debug, Clone, PartialEq)]
pub struct LanServer {
    pub address: SocketAddr,
    pub beacon: LanBeacon,
    /// Seconds since we last heard from it.
    pub age: f32,
}

#[derive(Resource, Default)]
pub struct LanBrowser {
    /// Set by the Join screen while it's open.
    pub active: bool,
    socket: Option<UdpSocket>,
    pub servers: HashMap<SocketAddr, LanServer>,
    /// Why we can't listen, if we can't.
    pub problem: Option<String>,
}

impl LanBrowser {
    /// Servers heard recently, sorted by name.
    pub fn list(&self) -> Vec<LanServer> {
        let mut v: Vec<LanServer> = self.servers.values().cloned().collect();
        v.sort_by(|a, b| a.beacon.name.cmp(&b.beacon.name));
        v
    }
}

fn listen(time: Res<Time>, mut browser: ResMut<LanBrowser>) {
    // Only tell the UI something changed when the list really changes, so
    // the Join screen isn't rebuilt every frame.
    let changed = update(browser.bypass_change_detection(), time.delta_secs());
    if changed {
        browser.set_changed();
    }
}

fn update(browser: &mut LanBrowser, dt: f32) -> bool {
    if !browser.active {
        let had = browser.socket.take().is_some() || !browser.servers.is_empty();
        browser.servers.clear();
        browser.problem = None;
        return had;
    }
    let mut changed = false;
    if browser.socket.is_none() && browser.problem.is_none() {
        match UdpSocket::bind(("0.0.0.0", LAN_DISCOVERY_PORT)) {
            Ok(s) => {
                let _ = s.set_nonblocking(true);
                browser.socket = Some(s);
            }
            Err(e) => {
                browser.problem = Some(format!("Can't look for games on this network ({e})"));
                changed = true;
            }
        }
    }
    for s in browser.servers.values_mut() {
        s.age += dt;
    }
    let before = browser.servers.len();
    browser.servers.retain(|_, s| s.age < 4.0);
    changed |= browser.servers.len() != before;

    let mut heard = Vec::new();
    if let Some(socket) = &browser.socket {
        let mut buf = [0u8; 1024];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            if let Some(beacon) = decode::<LanBeacon>(&buf[..n])
                && beacon.protocol == PROTOCOL_ID
            {
                heard.push((SocketAddr::new(from.ip(), beacon.port), beacon));
            }
        }
    }
    for (address, beacon) in heard {
        let entry = LanServer {
            address,
            beacon,
            age: 0.0,
        };
        let old = browser.servers.insert(address, entry.clone());
        changed |= old.is_none_or(|o| o.beacon != entry.beacon);
    }
    changed
}
