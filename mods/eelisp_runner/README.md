# Mods in EELisp

Write a mod as one text file in [EELisp](https://eelisp.app), no Rust or
build step needed. Put `something.el` in the mods folder and it's a mod
called `something`:

- **Single player:** the `mods` folder next to your worlds (on a Mac,
  `~/Library/Application Support/Ourcraft/mods`). Then switch it on for
  the worlds you want: **Play → Mods** next to a world, or **Mods** when
  making a new one. Mods you add start switched off, so a mod only runs
  where you choose.
- **A dedicated server:** its `--mods` folder. Every mod there runs.
- **Built into the game:** `mods/dist/`.

The game runs each script in its own copy of the EELisp runner
(`mods/runtimes/eelisp.wasm`), inside the same sandbox as every other mod:
a script can't touch files or the internet, and each event has a time
limit.

## Events

Define any of these and the game calls them:

| Function | When |
|---|---|
| `(on-start)` | the world is ready |
| `(on-join player name)` | someone joined |
| `(on-leave player)` | someone left |
| `(on-chat player text)` | a chat line. Return `true` if it was your command (then nobody else sees it) |
| `(on-tick tick)` | 20 times a second. Keep it short |
| `(on-block-change player x y z old new)` | someone is changing a block. Return `:deny` to stop it |
| `(on-fish-bite player rod x y z)` | a fish bit someone's line (`x y z` is the water). Return the catch: an item name, or `["name" count]`. Return nil for the game's usual catch |

If the script has a mistake, the game says so in the chat (starting with
`(!)`), as well as in the server log: when the world loads for a mistake in
the script itself, the first time it happens for one in an event.

The rest of the script runs once, when the world loads. That's where you
register blocks and set up your variables with `def`.

## Game functions

| Function | What it does |
|---|---|
| `(chat "text")` | a chat line for everyone |
| `(tell player "text")` | a chat line for one player |
| `(show-title "text" seconds [player])` | big text in the middle of the screen |
| `(set-hud player-or-nil ["line" …])` | the panel in the corner |
| `(players)` | everyone, as dicts `{:id :name :x :y :z}` |
| `(player-pos player)` | the block a player stands in, `(x y z)` |
| `(teleport player x y z)` | move a player |
| `(block-id "core:stone")` | a block's id, or nil |
| `(get-block x y z)` | the id of the block there |
| `(set-block x y z block)` | change one block (a name or an id). True if it worked |
| `(fill x1 y1 z1 x2 y2 z2 block)` | a whole box at once, much faster than `set-block` in a loop. Returns how many changed |
| `(ground-height x z)` | the y of the top solid block |
| `(register-block {:name "mod:thing" :display "Thing" :color [r g b] :light 0-15 :glass true})` | a new block. Only while the script first runs. It can have a `:picture` too (see below) |
| `(register-item {:name "mod:thing" :display "Thing" :color [r g b] :food 0-20 :heals 0-20 :cooks-into "mod:other" :stack 1-64 :fuel seconds})` | a new item. Only while the script first runs. An item whose name ends in `_rod` casts a fishing line |
| `(register-recipe {:pattern ["ab" "ba"] :key {:a "core:stick" :b "core:string"} :result "mod:thing" :count 1})` | a crafting recipe; or `{:ingredients ["core:bread" "mod:fish"] :result …}` for one in any arrangement. Only while the script first runs |
| `(give-item player "core:bread" 3)` | items into a player's inventory (count is 1 if left out) |
| `(take-item player "core:bread" 3)` | takes up to that many; returns how many it took |
| `(count-item player "core:bread")` | how many they have |
| `(random 6)`, `(chance 25)`, `(pick [a b c])` | 0 to 5; true 25 times in 100; one of them |
| `(world-spawn)` | where new players appear, `[x y z]` |
| `(spawn-mob "core:pig" x y z)` | an animal |
| `(sound "name" [x y z])` | a sound, for everyone or from a place |
| `(confetti x y z)`, `(hearts x y z)`, `(poof x y z)` | a little celebration |
| `(time-of-day)`, `(set-time-of-day t)` | 0 is midnight, 0.5 noon |
| `(log "text")` | a line in the server log (so does `println`) |

**Pictures.** Instead of `:color`, an item or block can have a picture
drawn with letters: `:picture` is a list of up to 16 rows of up to 16
letters, and `:colors` says what colour each letter is:

```lisp
(register-item {:name "fish:perch" :display "Puddle Perch"
                :picture ["...bbb.t" ".bbbbbtt" "bebbbbbt" "bwwwwbtt" ".wwwww.t"]
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}})
```

`.` and spaces are see-through (on a block, they're its `:color`). A
small picture is scaled up to fill the 16×16 square.

Everything else is plain EELisp: `defn`, `let`, `if`, `cond`, `for-each`,
`range`, strings, lists `[1 2 3]` and dicts `{:a 1}`. Note that
`(1 2 3)` is a function call: write lists with square brackets.

## Examples

- [`../tutorials/fishing`](../tutorials/fishing): a step-by-step tutorial
  for children (about 10 and up): a fishing mod with fish they draw,
  places to fish, rods, a fish market, cooking and a journal.

- [`examples/glow_garden.el`](examples/glow_garden.el): two blocks, a
  `/garden` command, and glow blocks only the gardener can break.
- [`examples/builder.el`](examples/builder.el): `/tower`, `/wall` and
  `/bigwall`, for comparing `set-block` in a loop with `fill`.

## How fast is it?

The language runs inside the sandbox, so it's slower than a Rust mod. From
`crates/mod_host/tests/eelisp.rs`, as a share of one event's time budget:
starting up 4%, 10 blocks 0.2%, 300 blocks one at a time 5.3%, 300 blocks
with `fill` 1.1%. Plenty for commands and game rules. For big builds use
`fill`.

## Building the runner

```sh
mods/eelisp_runner/build.sh   # writes mods/runtimes/eelisp.wasm
```

It gets EELisp from [eelisp-rs](https://github.com/santacroce-tech/eelisp-rs)
(pinned to one commit in `Cargo.toml`), with its default features off: just
the language, no SQLite or network. The built `mods/runtimes/eelisp.wasm` is
committed, so building the game doesn't need this step.
