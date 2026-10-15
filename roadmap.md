# Ourcraft roadmap

Where Ourcraft stands compared with the classic block-building game, and
what to build next. Milestones 1–10 (the original plan) are done; this file
picks up from there.

## What we have

| Area | Done |
|---|---|
| World | Infinite generated terrain; plains, forest, desert, snowy and mountain biomes; caves; trees; cacti; seas and lakes; beaches; bedrock floor; a seed per world |
| Blocks | 16 built-in (stone, dirt, grass, sand, log, leaves, planks, snowy grass, cactus, bedrock, gravel, cobblestone, glass, bricks, lamp, water), plus 9 from the Paint Blocks mod and 2 from the Snow Fun tutorial mod |
| Building | Break, place, pick block; 9-slot hotbar; 36-slot inventory; creative block palette |
| Game modes | Creative (fly, endless blocks) and survival (collect what you break; stone gives cobblestone, grass gives dirt); Splat Squad team game (mod) |
| Movement | Walk, sprint, jump, gravity, collision, swimming, flying (double-tap jump), auto-jump on touch screens |
| Looks | Day/night cycle, sun and moon, sunset colours, fog, ambient occlusion, see-through leaves and glass, glowing lamps |
| Saving | Several worlds, autosave, region files |
| Multiplayer | Dedicated server, single player through the same server, LAN discovery, direct connect, chat with commands, avatars with name tags, client prediction |
| Platform | macOS, Windows, Linux, iPad/iPhone, Android; touch and gamepad controls |
| Extras | WebAssembly mods, main menu, settings, sound effects, chat filter |

## Partly done

- **Lighting:** sunlight dims at night, but caves aren't dark, and lamps
  glow without lighting their surroundings.
- **Survival:** broken blocks go straight into the inventory (no dropped
  items), every block breaks instantly, and there are no tools.
- **Water:** still; it doesn't flow.
- **Sound:** effects for placing, breaking and the game mode; no
  footsteps, ambience or music.

## Missing

- Crafting (crafting table, furnace, recipes)
- Mobs: animals and monsters
- Health, hunger, fall damage and drowning outside Splat Squad
- Tools, armour, food
- Ores: coal, iron, gold, diamond
- Non-cube blocks: stairs, slabs, doors, fences, torches on walls
- Blocks with storage or behaviour: chests, furnaces, beds, signs
- Redstone-style circuits
- Falling sand and gravel; flowing water and lava
- Villages and other generated structures
- Other dimensions (an "underworld" and an "end" world)
- Weather (rain, snow, thunder) and clouds
- Farming (seeds, wheat, animals to feed)
- Experience points and achievements
- Maps and a compass

## Next milestones

Ordered for a young player: things to collect and make first, then
company in the world, then atmosphere. Each one should be playable before
moving on, like milestones 1–10. "Mod" means it can be built with the
existing mod API; "engine" means the game itself needs changes.

### 11. Ores and crafting

- Ore blocks in the world generator: coal, iron, gold, diamond (deeper
  means rarer). *Engine: world generation.*
- Dropped items: breaking a block spawns a small spinning item that you
  pick up by walking over it. *Engine: entities exist; add item pickup.*
- Crafting: a 2×2 grid in the inventory, plus a crafting table block for
  3×3, with recipes for planks, sticks, tables, torches, tools, and bricks
  from clay. *Engine: inventory UI and server-checked recipes. Mod API: let
  mods register recipes.*
- Tools (wood, stone, iron, diamond pickaxe, axe and shovel) and block
  hardness, so breaking takes time and the right tool is faster. *Engine:
  break progress with cracks shown on the block.*
- Furnace: smelt ore into ingots, and sand into glass. *Engine: block
  entities, meaning blocks with their own data.*

### 12. Friendly animals

- Simple mobs made of boxes (the avatar code already draws box models):
  sheep, pigs, chickens, cows, wandering and grazing. *Engine: server-side
  mob AI (wander, avoid water and cliffs) and walking on terrain with the
  shared physics.*
- Feeding and breeding; wool and eggs. *Can be a mod once mobs exist.*
- Mod API: let mods register mob types (model, speed, behaviour).

### 13. Real light

- Block light that spreads from torches, lamps and lava, plus skylight,
  computed per chunk and stored alongside the blocks. Caves become dark.
  *Engine: light propagation and remeshing with per-vertex light.*
- Torches: a non-cube block that can sit on walls.
- Option for players to turn off monster spawning in the dark (for later
  milestones).

### 14. Building blocks with shapes

- Stairs, slabs, fences, doors, trapdoors, ladders. *Engine: non-cube
  block shapes in meshing, collision and raycasts.*
- Chests (storage UI, shared in multiplayer) and signs (text on blocks).
  *Engine: block entities from milestone 11.*
- Beds: sleep through the night, and set where you respawn.

### 15. Living world

- Survival needs: health, hunger, food, fall damage, drowning, with
  gentle kid-friendly defaults (and a "peaceful" option). *Engine; Splat
  Squad already has health UI.*
- Gentle monsters at night, cartoon style (no gore), easy to switch off.
  *Engine mob AI from milestone 12; behaviour can be a mod.*
- Flowing water and lava; falling sand and gravel. *Engine: block
  updates.*

### 16. Atmosphere

- Weather (rain, snow in cold biomes) and clouds.
- Footsteps per block type, ambient sounds (birds, water, wind) and a few
  calm music loops.
- Particles when breaking blocks.

### Later ideas

- Villages and other structures in world generation.
- Simple circuits (switches, wires, lamps, doors).
- Farming: seeds, wheat, bread.
- Maps and a compass.
- Achievements to guide new players ("Make a crafting table!").
- More game modes as mods: a racing course, hide and seek, a building
  contest with voting.
- Player skins: choose colours and a face, drawn as box models.

## Housekeeping

- Check the darker colours seen on the Android emulator against a real
  device.
- Typing on phones and tablets (show the on-screen keyboard for chat and
  world names).
- Ask Apple for the multicast permission so LAN discovery works on iOS.
- Move to Bevy 0.20 once the networking crates support it.
- Sunset sky looks muddy brown; make the colours warmer and brighter.
- The sun is drawn as a clipped triangle instead of a square.
