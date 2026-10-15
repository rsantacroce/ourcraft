# Ourcraft roadmap

Where Ourcraft stands compared with the classic block-building game, and
what to build next. Milestones 1–10 (the original plan) and 11–15 are done; this file
picks up from there.

## What we have

| Area | Done |
|---|---|
| World | Infinite generated terrain; plains, forest, desert, snowy and mountain biomes; caves; trees; cacti; seas and lakes; beaches; bedrock floor; a seed per world |
| Blocks | 26 built-in (stone, dirt, grass, sand, log, leaves, planks, snowy grass, cactus, bedrock, gravel, cobblestone, glass, bricks, lamp, water, coal/iron/gold/diamond ore, clay, crafting table, furnace, iron/gold/diamond blocks, wool), plus 9 from the Paint Blocks mod and 2 from the Snow Fun tutorial mod |
| Items | Sticks, coal, iron and gold ingots, diamonds, clay balls, bricks, shears, eggs, apples; wooden, stone, iron and diamond pickaxes, axes and shovels that wear out |
| Building | Break, place, pick block; 9-slot hotbar; 36-slot inventory; creative palette of every item; stairs and slabs (wood, cobblestone, stone, brick), fences that join up, doors and trapdoors that open, ladders to climb, chests, signs you write on, beds that skip the night |
| Crafting | 2×2 grid in the inventory, 3×3 on a crafting table, 25 recipes (mods can add more); furnace that smelts ore, sand, clay and cobblestone with coal or wood |
| Game modes | Creative (fly, endless blocks) and survival (blocks take time to break, the right tool is faster, drops land as items you pick up; health, hunger and breath; falls, drowning and lava hurt; fainting sends you back to bed with your things; gentle monsters at night; `/peaceful`); Splat Squad team game (mod) |
| Living world | Water and lava flow (and lava meeting water turns to stone); sand and gravel fall; lava lakes deep underground |
| Movement | Walk, sprint, jump, gravity, collision, swimming, flying (double-tap jump), auto-jump on touch screens |
| Looks | Day/night cycle, sun and moon, sunset colours, fog, ambient occlusion, see-through leaves and glass; real light: sunlight that doesn't reach into caves, and warm light from torches and lamps, smooth across corners |
| Animals | Sheep, pigs, cows and chickens that wander, follow you when you hold an apple, have lambs, piglets, calves and chicks; shear sheep for wool, chickens lay eggs; pet them for hearts; `/summon` in creative; mods can add their own |
| Saving | Several worlds, autosave, region files |
| Multiplayer | Dedicated server, single player through the same server, LAN discovery, direct connect, chat with commands, avatars with name tags, client prediction |
| Platform | macOS, Windows, Linux, iPad/iPhone, Android; touch and gamepad controls |
| Extras | WebAssembly mods, main menu, settings, sound effects, chat filter |

## Partly done

- **Sound:** effects for placing, breaking and the game mode; no
  footsteps, ambience or music.

## Missing

- Armour, swords, more food (bread with farming)
- Redstone-style circuits
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

### 11. Ores and crafting ✅ done

Ores (coal, iron, gold, diamond, rarer deeper down) and clay under water;
items, including tools in four tiers that wear out; break times with
cracks; drops that pop out and get picked up (Q throws items); crafting in
the inventory and on a crafting table; furnaces kept and saved by the
server; recipes from mods (`register_recipe`). Old saves upgrade on their
own (inventories are now stored by item name).

Not quite as planned: the player's own game checks crafting recipes, not
the server (the server trusts players' inventories, as before). Mods can
add recipes but not yet new items.

What was planned:


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

### 12. Friendly animals ✅ done

Sheep, pigs, cows and chickens made of boxes, with legs that swing as
they walk; they wander, step up blocks, and keep away from water and
cliffs. Hold an apple (a rare drop from leaves) and they follow you; feed
two and they have a baby that grows up. Shears give wool (it grows back),
chickens lay eggs, and a pat with an empty hand makes them hop with
hearts. `/summon sheep` in creative. Mods can add animals
(`register_mob`, `spawn_mob`).

Not quite as planned: animals aren't saved; they appear near players and
go away when everyone leaves (ones a mod made stay). Food is apples for
every animal until farming adds wheat.

What was planned:


- Simple mobs made of boxes (the avatar code already draws box models):
  sheep, pigs, chickens, cows, wandering and grazing. *Engine: server-side
  mob AI (wander, avoid water and cliffs) and walking on terrain with the
  shared physics.*
- Feeding and breeding; wool and eggs. *Can be a mod once mobs exist.*
- Mod API: let mods register mob types (model, speed, behaviour).

### 13. Real light ✅ done

Sky light and block light (0–15), worked out per chunk while meshing from
the 27 chunks around it (light never goes further than 15 blocks, so this
is exact), with smooth light at each corner. Caves and covered places are
dark, torches (on floors and walls; 4 from coal and a stick) and lamps
light their surroundings warmly, and only sky light fades at night.
Torches fall off when the block they hang on breaks. Blocks can now have
shapes made of boxes (drawn, aimed at and placed correctly), ready for
milestone 14.

Not quite as planned: animals and dropped items aren't darkened in caves
yet (only by night). The monster-spawning switch comes with the monsters
(milestone 15).

What was planned:


- Block light that spreads from torches, lamps and lava, plus skylight,
  computed per chunk and stored alongside the blocks. Caves become dark.
  *Engine: light propagation and remeshing with per-vertex light.*
- Torches: a non-cube block that can sit on walls.
- Option for players to turn off monster spawning in the dark (for later
  milestones).

### 14. Building blocks with shapes ✅ done

Stairs and slabs in four materials, fences that join to each other and to
walls, doors (two blocks tall) and trapdoors that open and close, ladders
(jump to climb), chests with 27 slots shared by everyone, signs with four
lines of text drawn in a little pixel font (rude words filtered), and beds
that set where you wake up and skip the night once everyone's in bed.
Shapes collide box by box, and you walk up slabs and stair steps without
jumping. Each shape faces the way you look or the wall you click.

Not quite as planned: one kind of fence and door (wood); no half-open
fence gates or coloured beds yet. Sleeping doesn't show you lying down.

What was planned:


- Stairs, slabs, fences, doors, trapdoors, ladders. *Engine: non-cube
  block shapes in meshing, collision and raycasts.*
- Chests (storage UI, shared in multiplayer) and signs (text on blocks).
  *Engine: containers from milestone 11 (furnaces) make chests easy.*
- Beds: sleep through the night, and set where you respawn.

### 15. Living world ✅ done

Health, hunger and breath with gentle rules: falls hurt only from high up
(water breaks any fall), lava stings, hunger heals you when full and can't
make you faint by itself, and at zero health you faint and wake up in your
bed (or at the start) with all your things. Food: apples, fried eggs
(cook an egg) and golden apples (they heal). Two cartoon monsters come out
near survival players at night (the Grumble walks, the Bouncer hops),
bonk you for a heart or half a heart, get bopped away with a few hits
(tools hit harder), and puff away at sunrise. `/peaceful` turns off
monsters and hunger for the world. Water and lava flow (7 and 3 blocks,
lava more slowly), dry up without a source, and make stone where they
meet; sand and gravel fall; lava lakes fill the deepest caves. Water and
lava sources are in the creative palette.

Not quite as planned: no bucket yet, so in survival water only flows out
of lakes and the sea; health and hunger reset when you rejoin; monsters
appear at night on the surface, not in dark caves by day (the server
doesn't know light levels).

What was planned:


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
