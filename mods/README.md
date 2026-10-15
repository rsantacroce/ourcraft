# Ourcraft mods

Mods add blocks, tools, entities and whole game modes to Ourcraft without
changing the game itself. They're small Rust programs compiled to
WebAssembly and run by the server in a sandbox: a mod can only talk to the
game through the mod API, never to your files or the internet.

## Build the mods

```sh
rustup target add wasm32-unknown-unknown   # once
./build.sh
```

This writes `dist/*.wasm`. The game bundles everything in `dist/` into its
own binary when it's built (so phones and tablets get the mods too).
Dedicated servers also load extra mods from a `mods/` folder next to them
(`ourcraft-server --mods FOLDER` to change it).

## Mods in this folder

- **paint_blocks**: eight paint colours, a glowing rainbow lamp, and a
  `/rainbow` chat command. The best place to start reading.
- **team_shooter**: *Splat Squad*, a two-team game mode. Start a world with
  it using `--mode team_shooter`.

## Write your own

1. Copy `paint_blocks` to a new folder and add it to `members` in
   `Cargo.toml`.
2. Implement `Mod::on_event`. The events (`Event` in
   `crates/mod_api/src/lib.rs`) tell you what's happening: `Init` (register
   your blocks, textures, models and tools here), `Start`, `Tick` (20 times a
   second), `PlayerJoined`, `BlockChange` (reply `Deny` to stop it), `Chat`
   (reply `Handled` for your commands), `UseTool`, `ProjectileHit`…
3. Call functions in `ourcraft_mod_api::guest` to change the game:
   `set_block`, `chat`, `teleport`, `spawn_entity`, `show_title`, …
4. Finish with `ourcraft_mod_api::export_mod!(YourMod);`
5. Run `./build.sh`, then rebuild/restart the game.

Textures are 16×16 pixels. `pixel_art` turns 16 strings of 16 characters
plus a palette into a texture, which is a fun way to draw them by hand.

## Rules mods run under

- No files, network, clock or randomness from the system: only the API.
  (Need random numbers? Use a simple generator seeded from the tick.)
- Each event may use a limited amount of work ("fuel"). A mod stuck in an
  endless loop is stopped, and switched off after three failures.
- At most 64 MiB of memory per mod.
- Mods are checked for the API version they were built with
  (`API_VERSION`). Rebuild your mods after updating the game.
