# Ourcraft

A blocky sandbox game in Rust: infinite generated worlds to explore and
build in, survival with ores, tools, crafting and furnaces, friendly
animals you can keep, gentle night (and cave) monsters, real light, flowing
water you can carry in a bucket, villages, gentle goals to guide new
players, the **Cloud Isles** (floating islands in a dreamy sky, through a
portal you build), a skin maker, multiplayer, mods, and two game modes: the
cartoon team game **Splat Squad** and **Build Contest**. Runs on macOS, Windows, Linux, iPad/iPhone and Android.

**Website and player manual:** https://rsantacroce.github.io/ourcraft-site/

## Play

**Download** the game for macOS, Windows or Linux from the
[latest release](https://github.com/rsantacroce/ourcraft/releases/latest)
(or the website), or build it from source:

```sh
cargo run --release            # main menu
```

The first build takes a few minutes; after that it's quick.

From the menu: **Play** (your worlds, sorted by last played, name or
date made, with a Delete button each; or make a new one and pick *Build*,
*Splat Squad* or *Build Contest*), **Join a friend** (games on your
network, or type an address), **Settings**.

Shortcuts from the command line:

```sh
cargo run --release -- --world "My World"                 # straight into a world
cargo run --release -- --world Arena --mode team_shooter  # a Splat Squad world
cargo run --release -- --world Plots --mode build_contest  # a Build Contest world
cargo run --release -- --world "My World" --lan           # let your network join
cargo run --release -- --connect 192.168.1.20 --name Sam  # join a server
cargo run --release -- --world Test --lag 150 --loss 5    # simulate a bad network
```

### Controls

| Keyboard / mouse | Gamepad | Touch |
|---|---|---|
| WASD move, mouse look | left stick, right stick | left thumb joystick, drag to look |
| Space jump (double-tap: fly) | A | Jump button (auto-jump steps) |
| Shift down, Ctrl run | B, left stick click | Down button, push stick fully |
| Left click break / use | right trigger | press and hold |
| Right click place | left trigger | tap |
| Middle click pick block (can be switched off in Settings) | D-pad up | |
| 1–9, wheel: hotbar (the wheel can be switched off in Settings) | bumpers | tap a slot |
| E inventory and crafting, F fly, G creative/survival | Y, X | Bag, Fly buttons |
| Q drop one (Ctrl+Q: the stack) | | |
| Right click a crafting table / furnace: use it | left trigger | tap |
| F5 camera: first person, behind, in front | D-pad down | View button |
| Enter chat, Esc menu | | Chat, Menu buttons |
| F3 info text, F7 simulate lag, T skip an hour, [ ] view distance | | |

### Building tools (creative)

Type these in chat (Enter). Leave out the block to use the one you're holding.

| Command | What it does |
|---|---|
| `/pos1`, `/pos2` | Mark the corners of a box at the block you're looking at |
| `/fill [block]`, `/clear` | Fill the box (`/fill stone`, `/fill oak_planks`), or empty it |
| `/walls [block]`, `/box [block]` | Just the four walls, or a hollow box |
| `/replace <old> [new]` | Swap one block for another inside the box |
| `/sphere <size> [block]` | A ball where you're looking |
| `/copy`, `/paste` | Copy the box and paste it where you're standing (`/paste solid` skips air) |
| `/undo`, `/sel`, `/build` | Take back the last change (up to 8), show the box, help |

Chests, signs, frames and banners are never overwritten.

Stuck somewhere? `/spawn` takes you back to your bed (or the start of
the world), in any mode. If you end up inside a block, the game lifts you
out after a second.

### Goals

In survival, nineteen gentle goals guide you through the game, from
*Get some wood* and *Make a crafting table* to *Find a village*, *Find a
diamond* and *Visit the Cloud Isles*. Each one you reach gets a cheer and
a hint for the next. Type `/goals` to see what's done and what's left.

### Villages

Out on open plains, deserts and snowfields you'll come across villages:
four to seven little houses (oak, sandstone or pine) round a well with a
lamp post, joined by gravel paths. Each house has a door, windows, a bed,
a crafting table and a torch.

### Swords, slingshot and shield

Monsters are bopped away, never hurt for real. Bare hands bop for 2,
tools a bit more, and **swords** most of all (wood 5, stone 6, iron 7,
diamond 8; two of the material over a stick). The **slingshot** (two
sticks and a string over a stick) flicks a pebble that bops whatever it
hits up to 24 blocks away, no ammo needed. A **shield** (planks round an
iron ingot) in your hand halves monsters' bonks.

### Build Contest

A game mode for friends: everyone gets a plot of their own and the same
secret theme ("A rocket ship", "A house for a cat"…), builds it against
the clock, then everyone visits each build and votes with badges from
*Meh* to *WOW!*. The most points wins. Type `/start` to begin a round
(`/start 3` for three minutes of building) and `/theme` to see the theme
again. During a round you can only build on your own plot.

### The Cloud Isles

1. Craft **sunstone**: eight glass round a gold ingot makes four.
2. Build a standing frame of sunstone round a space at least 2 wide and
   3 tall (10 blocks; the corners don't matter).
3. Use a **feather** on the frame: it fills with a shimmering cloud portal.
4. Stand in it for a moment. Whoosh!

On the other side you come out in a portal (one is built for you if there
isn't one nearby), so the way home is always there. Falling off an island
doesn't hurt: you float down through the clouds and land at home. No
monsters, no rain, and landing on a cloud never hurts.

### Skins

Pick how you look under **Settings → Skin**: six built-in skins, one you
make yourself, or your own picture. **Make your own skin** (in Settings)
picks skin tone, hair colour and length, eyes, a face (smile, grin,
surprised, freckles or glasses), shirt, trousers, shoes and a hat, with a
preview and a *Surprise me!* button; everyone you play with sees it. Drop 64×64 skin PNGs (the classic layout, so existing skin files
work; old 64×32 ones too) into the `skins` folder next to `settings.txt`
(on a Mac, `~/Library/Application Support/Ourcraft/skins`) and they appear
in the list. Other players see your skin in multiplayer. Press F5 to see
yourself.

## Dedicated server

```sh
cargo run --release -p ourcraft_server --bin ourcraft-server -- \
    --world world --port 41700 [--mode team_shooter|build_contest] [--survival]
```

Worlds are saved in `./worlds/`, extra mods are loaded from `./mods/`.
Ctrl+C saves and stops. The server is headless (no window, minimal Bevy).

## Making a release

Push a version tag and GitHub builds the game and server for macOS
(Apple Silicon + Intel), Windows and Linux and attaches them to a release
(`.github/workflows/release.yml`); the website's download buttons always
point at the latest one:

```sh
git tag v0.1.0 && git push origin v0.1.0
```

## Phones and tablets

- **iPad/iPhone:** `cd mobile/ios && make run` builds and starts it in the
  iPad simulator. For a real device, open `mobile/ios/ourcraft_mobile.xcodeproj`
  in Xcode, choose your team under *Signing*, and press Run. Change the
  bundle id `org.ourcraft.game` to your own.
- **Android:** `mobile/android/build.sh` builds `app-release.apk` (needs the
  Android SDK + NDK, `rustup target add aarch64-linux-android` and
  `cargo install cargo-ndk`). Install with `adb install -r …`.

Typing works on phones and tablets: the on-screen keyboard comes up for
chat (the **Chat** button, next to Menu) and for menu fields like world
names and seeds. While you type, chat moves to the top of the screen and
the field you're typing in shows above the keyboard.

## Mods

Mods are WebAssembly modules that run sandboxed on the server (no files,
no network). See the
[Modding Guide](https://rsantacroce.github.io/ourcraft-site/modding.html) and
[mods/README.md](mods/README.md). Build them with
`mods/build.sh`; the game bundles `mods/dist/*.wasm`.

## How it's built

```
crates/common    shared: blocks, chunks, world generation, physics,
                 prediction, saves, network protocol (no Bevy)
crates/client    the game: rendering, meshing, input, UI, sound
crates/server    authoritative server: world, players, entities, mods;
                 dedicated binary or embedded for single player
crates/mod_api   the versioned mod API (shared by game and mods)
crates/mod_host  runs mods with wasmtime (fuel + memory limits)
mods/            paint_blocks (example), snow_fun (the guide's tutorial),
                 team_shooter (Splat Squad), build_contest (Build Contest)
mobile/          iOS (Xcode) and Android (Gradle) launchers
```

- **World:** 16³ chunks in columns 256 blocks tall, palette-compressed;
  greedy meshing with ambient occlusion on background threads; a texture
  array with mipmaps; fog; day/night.
- **Dimensions:** the server stacks the Cloud Isles 512 blocks below the
  overworld in one world, so water, plants, circuits and animals work in
  both unchanged. Each game only sees its own dimension at the usual
  heights: the server moves positions on the way in and out, and leaves
  out anything (blocks, animals, players) from the other one.
- **Saves:** region files of 32×32 columns, LZ4-compressed, only edited
  columns stored; block ids saved by name so mods can come and go. The
  Cloud Isles have their own, in `dimensions/cloud_isles/region/`.
- **Network:** renet over UDP (netcode handshake). Single player uses the
  same server through an in-memory link. Movement is server-authoritative
  with client-side prediction and reconciliation; other players are
  interpolated.
- **Mods:** postcard-encoded messages over a single `ourcraft.call` import;
  Cranelift on desktop/Android, the Pulley interpreter on iOS.

## Tests

```sh
cargo test --workspace     # unit tests + end-to-end server tests (bots)
cargo test -p ourcraft_server --test build_contest -- --ignored   # a whole contest round (slow)
```

## Versions

Rust 1.96, Bevy 0.19.1 (pinned: the networking ecosystem wasn't on 0.20 yet),
renet 2.0, wasmtime 49.0.3, fastnoise-lite 1.1, postcard 1.1, lz4_flex 0.14.

## License

MIT. See [LICENSE](LICENSE). All textures, sounds, models and names in the
game are original.
