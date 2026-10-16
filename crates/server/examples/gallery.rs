//! Builds a gallery of every block around the spawn point of a running
//! server, for checking the art by eye:
//!
//! ```text
//! cargo run -p ourcraft_server --bin ourcraft-server -- --world Gallery
//! cargo run -p ourcraft_server --example gallery
//! cargo run -p ourcraft_client -- --connect 127.0.0.1
//! ```
//!
//! The spawn point is cleared into a room with a stone floor; the blocks
//! stand in walls on all four sides (plants in a garden row in front).

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::block::{BlockId, BlockRegistry};
use ourcraft_common::farming::StageKind;
use ourcraft_common::protocol::{
    self, Channel, ClientMessage, DEFAULT_PORT, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
};
use ourcraft_server::net::connection_config;
use renet::RenetClient;
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

/// Marks a change made holding a hoe (the top bit of the block id).
const HOE: u16 = 1 << 15;

/// Half the room's width; the walls stand just outside it.
const HALF: i32 = 12;
const WALL_HEIGHT: i32 = 10;

struct Bot {
    client: RenetClient,
    transport: NetcodeClientTransport,
    received: Vec<ServerMessage>,
}

impl Bot {
    fn connect(server: SocketAddr, id: u64) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let auth = ClientAuthentication::Unsecure {
            protocol_id: PROTOCOL_ID,
            client_id: id,
            server_addr: server,
            user_data: None,
        };
        Self {
            client: RenetClient::new(connection_config()),
            transport: NetcodeClientTransport::new(now, auth, socket).unwrap(),
            received: Vec::new(),
        }
    }

    fn send(&mut self, msg: &ClientMessage) {
        self.client
            .send_message(Channel::Reliable, protocol::encode(msg));
    }

    /// Exchange packets for a while, collecting messages.
    fn pump(&mut self, dt: Duration) {
        self.client.update(dt);
        let _ = self.transport.update(dt, &mut self.client);
        for ch in [Channel::Reliable, Channel::Unreliable] {
            while let Some(b) = self.client.receive_message(ch) {
                if let Some(m) = protocol::decode::<ServerMessage>(&b) {
                    self.received.push(m);
                }
            }
        }
        let _ = self.transport.send_packets(&mut self.client);
    }

    fn has(&self, f: impl Fn(&ServerMessage) -> bool) -> bool {
        self.received.iter().any(f)
    }
}

fn run_until(bot: &mut Bot, secs: f32, done: impl Fn(&Bot) -> bool) -> bool {
    let start = Instant::now();
    let step = Duration::from_millis(10);
    while start.elapsed().as_secs_f32() < secs {
        bot.pump(step);
        if done(bot) {
            return true;
        }
        std::thread::sleep(step);
    }
    false
}

fn main() {
    let addr: SocketAddr = format!("127.0.0.1:{DEFAULT_PORT}").parse().unwrap();
    let mut bot = Bot::connect(addr, 9_999);
    assert!(
        run_until(&mut bot, 5.0, |b| b.client.is_connected()),
        "no server"
    );
    bot.send(&ClientMessage::Hello {
        name: "Gallery".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(run_until(&mut bot, 20.0, |b| b
        .has(|m| matches!(m, ServerMessage::Welcome { .. }))));
    let (feet, content) = bot
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { state, content, .. } => {
                Some((state.position, content.clone()))
            }
            _ => None,
        })
        .unwrap();
    // Let the columns arrive before building in them.
    run_until(&mut bot, 3.0, |_| false);
    let reg = BlockRegistry::from_defs(content.blocks.clone()).unwrap();
    let by = |n: &str| reg.by_name(n).unwrap().0;
    let (fx, fy, fz) = (
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    );
    bot.send(&ClientMessage::GameMode { creative: true });
    run_until(&mut bot, 0.3, |_| false);
    // Each change has a phase: the server checks edits against the world as
    // it was at the start of its tick, so clearing and then building in
    // the same spot need a pause in between.
    let mut queue: Vec<(u8, [i32; 3], u16)> = Vec::new();
    // The room: a stone floor and air, a bit beyond the walls too.
    // (Blocks only go into empty space, so clear first.)
    for x in -HALF - 2..=HALF + 2 {
        for z in -HALF - 2..=HALF + 2 {
            for y in -1..WALL_HEIGHT + 3 {
                queue.push((0, [fx + x, fy + y, fz + z], 0));
            }
            queue.push((1, [fx + x, fy - 1, fz + z], by("core:stone")));
        }
    }
    // Where each wall's blocks go, side by side, bottom row first.
    let sides = |i: usize| -> [i32; 3] {
        let per_side = (2 * HALF + 1) as usize;
        let per_wall = per_side * WALL_HEIGHT as usize;
        let (wall, k) = (i / per_wall, i % per_wall);
        let (along, up) = ((k % per_side) as i32 - HALF, (k / per_side) as i32);
        let d = HALF + 1;
        match wall % 4 {
            0 => [fx + along, fy + up, fz - d],
            1 => [fx + d, fy + up, fz + along],
            2 => [fx - along, fy + up, fz + d],
            _ => [fx - d, fy + up, fz - along],
        }
    };
    let skip = |name: &str| {
        name.contains("water")
            || name.contains("lava")
            || name.contains("fire")
            || name == "core:air"
    };
    let (mut walls, mut garden) = (0usize, 0usize);
    // Banners get a few patterns painted on at the end.
    let mut banners: Vec<[i32; 3]> = Vec::new();
    for (id, def) in reg.iter() {
        if id == BlockId(0) || skip(&def.name) {
            continue;
        }
        if reg.sits_on_ground(id) {
            // The garden: a row on the floor just inside the walls.
            let per_side = (2 * HALF - 1) as usize;
            let (side, k) = (garden / per_side, (garden % per_side) as i32 - HALF + 1);
            let d = HALF - 1 - 2 * (side / 4) as i32;
            let at = match side % 4 {
                0 => [fx + k, fy, fz - d],
                1 => [fx + d, fy, fz + k],
                2 => [fx - k, fy, fz + d],
                _ => [fx - d, fy, fz - k],
            };
            let crop = reg
                .stage(id)
                .is_some_and(|s| matches!(s.kind, StageKind::Crop | StageKind::Stem { .. }));
            let below = [at[0], at[1] - 1, at[2]];
            queue.push((2, below, 0));
            if crop {
                // Farmland is dug with a hoe.
                queue.push((3, below, by("core:dirt")));
                queue.push((4, below, by("core:farmland") | HOE));
            } else {
                queue.push((3, below, by("core:grass")));
            }
            queue.push((5, at, id.0));
            garden += 1;
        } else {
            let at = sides(walls);
            queue.push((5 + (at[1] - fy) as u8, at, id.0));
            if reg.variant_kind(id) == Some(ourcraft_common::shapes::VariantKind::Banner) {
                banners.push(at);
            }
            walls += 1;
        }
    }
    // A little scene in the middle, for the things that join up: wire
    // from a spark block round a corner to a lamp (and a wire on its own),
    // rails up a ramp onto a step, ferns, a big chest and a dispenser.
    let mut scene = |phase: u8, dx: i32, dy: i32, dz: i32, name: &str| {
        queue.push((phase, [fx + dx, fy + dy, fz + dz], by(name)));
    };
    scene(5, -5, 0, -3, "core:spark_block");
    for dx in -4..=0 {
        scene(5, dx, 0, -3, "core:spark_wire");
    }
    scene(5, 0, 0, -2, "core:spark_wire");
    scene(5, 0, 0, -1, "core:spark_wire");
    scene(5, 1, 0, -1, "core:spark_lamp");
    scene(5, -3, 0, 1, "core:spark_wire");
    for dx in -4..=-1 {
        scene(5, dx, 0, 3, "core:rail");
    }
    scene(5, 0, 0, 3, "core:stone");
    scene(5, 1, 0, 3, "core:stone");
    scene(6, 0, 1, 3, "core:rail");
    scene(6, 1, 1, 3, "core:rail");
    for dx in 3..=5 {
        scene(2, dx, -1, -4, "core:air");
        scene(3, dx, -1, -4, "core:grass");
        scene(5, dx, 0, -4, "core:fern");
    }
    scene(5, 3, 0, 0, "core:chest");
    scene(5, 4, 0, 0, "core:chest");
    scene(5, 3, 0, 2, "core:dispenser");
    scene(5, 4, 0, 2, "core:lectern");
    println!(
        "{} blocks in the walls, {} in the garden, {} changes",
        walls,
        garden,
        queue.len()
    );
    // The server only lets us build within reach, so hop (/tp) along,
    // hovering a little above and inside the room from what we build.
    let hoe = content
        .items
        .iter()
        .position(|i| i.name == "core:wood_hoe")
        .unwrap() as u16;
    let mut here = [i32::MAX; 3];
    queue.sort_by_key(|q| q.0);
    let mut phase = 0;
    for (n, (p, pos, block)) in queue.iter().enumerate() {
        if *p != phase {
            phase = *p;
            run_until(&mut bot, 0.4, |_| false);
        }
        let near = (0..3).all(|i| (pos[i] as i64 - here[i] as i64).abs() <= 2);
        if !near {
            here = *pos;
            let inward = |v: i32, c: i32| (c - v).signum() * 2;
            let at = [
                pos[0] + inward(pos[0], fx),
                pos[1] + 2,
                pos[2] + inward(pos[2], fz),
            ];
            bot.send(&ClientMessage::Chat {
                text: format!("/tp {}.5 {} {}.5", at[0], at[1], at[2]),
            });
        }
        bot.send(&ClientMessage::SetBlock {
            pos: *pos,
            block: block & !HOE,
            held: if block & HOE != 0 { hoe } else { 0 },
        });
        if n % 100 == 99 {
            run_until(&mut bot, 0.05, |_| false);
        }
    }
    run_until(&mut bot, 1.0, |_| false);
    let dye = |n: &str| {
        content
            .items
            .iter()
            .position(|i| i.name == n)
            .map(|i| i as u16)
    };
    let dyes: Vec<u16> = ["core:red_dye", "core:yellow_dye", "core:blue_dye"]
        .iter()
        .filter_map(|n| dye(n))
        .collect();
    for (i, at) in banners.iter().enumerate() {
        bot.send(&ClientMessage::Chat {
            text: format!(
                "/tp {}.5 {} {}.5",
                at[0] - (at[0] - fx).signum() * 2,
                at[1] + 1,
                at[2] - (at[2] - fz).signum() * 2
            ),
        });
        for d in dyes.iter().cycle().skip(i).take(1 + i % 3) {
            bot.send(&ClientMessage::UseBlock { pos: *at, held: *d });
        }
        run_until(&mut bot, 0.05, |_| false);
    }
    run_until(&mut bot, 2.0, |_| false);
    println!("Done; the gallery is round the spawn point at {fx} {fy} {fz}.");
}
