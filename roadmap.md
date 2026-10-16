# Ourcraft roadmap

Where Ourcraft stands compared with the classic block-building game, and
what to build next. Milestones 1–10 (the original plan) and 11–16 are done; this file
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
| Atmosphere | Drifting clouds; rain (snow where it's snowy) that greys the sky and stays outside; footsteps that sound like the ground; birds, water and wind; three calm music pieces now and then (can be switched off); bits flying off broken blocks |
| Extras | WebAssembly mods, main menu, settings, sound effects, chat filter |

## Partly done


## Missing

See milestones 17–24 for the full item list. Also still missing:

- Villages and other generated structures
- Experience points and achievements
- Thunder (left out on purpose, for now)
- Swords (left out on purpose: bopping monsters is enough)

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
add recipes and, since the gap fixes below, items (`register_item`).

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

Not quite as planned: food is apples for every animal until farming adds
wheat. (Animals weren't saved at first; see the gap fixes below.)

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

Not quite as planned: the monster-spawning switch came with the monsters
(milestone 15, `/peaceful`). (Animals and dropped items were only
darkened by night at first; see the gap fixes below.)

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

Not quite as planned: one kind of fence and door (wood), and no coloured
beds (that needs dyes). Other players still see a sleeper standing up.
(Fence gates, lying down in bed and writing signs on touch screens came
with the gap fixes below.)

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

Not quite as planned: the bucket, saved health, cave monsters and eating
on touch screens came a little later (see the gap fixes below).

What was planned:


- Survival needs: health, hunger, food, fall damage, drowning, with
  gentle kid-friendly defaults (and a "peaceful" option). *Engine; Splat
  Squad already has health UI.*
- Gentle monsters at night, cartoon style (no gore), easy to switch off.
  *Engine mob AI from milestone 12; behaviour can be a mod.*
- Flowing water and lava; falling sand and gravel. *Engine: block
  updates.*

### 16. Atmosphere ✅ done

Blocky clouds drift high up (greyer and thicker in the rain). The server
brings rain now and then (`/weather rain`, `/weather clear`); it greys
and dims the sky, falls only where the sky is open, sounds like rain, and
turns to snowflakes where the ground is snowy. Footsteps sound like grass,
stone, wood, sand or snow; birds sing by day near trees, water burbles,
wind blows high up. Three calm music pieces (synthesized, like every
sound) play every few minutes; there's a Music switch in Settings. Bits
fly off broken blocks in the block's colour.

Not quite as planned: no thunder (gentler for little ones).

What was planned:


- Weather (rain, snow in cold biomes) and clouds.
- Footsteps per block type, ambient sounds (birds, water, wind) and a few
  calm music loops.
- Particles when breaking blocks.

### Gap fixes ✅ done

The "not quite as planned" bits from milestones 11–16, filled in:

- **Health and hunger are saved** with the player (player files are now
  version 4; older ones upgrade on their own). Someone who left while
  fainting comes back with a little health.
- **Animals you look after are saved** (`animals.dat`): ones you've
  petted, fed, sheared, bred or summoned stay where they are and come back
  when the world opens. Wild ones still come and go, so the world doesn't
  fill up. Kept animals stop having babies at 200.
- **Bucket** (iron ingots in a V): scoop up still water or lava, pour it
  out somewhere else.
- **Monsters in dark caves**, day or night, and never where torches or
  lamps light things up (on the surface too). Cave monsters stay in their
  caves in the morning. The server estimates light from glowing blocks
  nearby (walls aren't counted, so it errs on the bright side).
- **Animals, dropped items and mod entities are lit like the terrain**:
  dark in caves and at night, bright next to a torch.
- **Touch screens:** an Eat button appears when you hold food in survival,
  and signs get an on-screen keyboard.
- **Fence gates** (sticks and planks: `#P# / #P#`) open and close, and
  fences join up to them.
- **Lying down in bed:** the view lies on the pillow looking up; moving
  gets you up.
- **Mods can add items** (`register_item` with an `ItemSpec`: picture,
  stack size, food, fuel, smelting). Snow Fun adds a snow cone.
- Fixed on the way: players in creative or in game modes were sent their
  (empty) health bars every tick.

Still left, on purpose: the server trusts players' crafting (as it trusts
their inventories), and there's no thunder.

### The big item list (milestones 17–24)

Minecraft has well over a thousand items, most of them colour and wood
variants of each other. The batches below cover the *kinds* of things it
has, so Ourcraft ends up with a similar range, about 150–200 core items.
Plain words (bread, bucket, boat) are fine; anything that's another game's
own invention gets an original name and look (see the note at the end).
Each batch should be playable on its own, with recipes, pictures and a
manual page, like the earlier milestones.

### 17. Farming ✅ done

Hoes (four tiers) till dirt and grass into farmland, which stays wet
within four blocks of water (dry, bare farmland turns back to dirt).
Wheat, carrots, potatoes and beetroots grow in stages on farmland (faster
when wet) and give their harvest when broken: young plants give back the
seed, ripe ones more. Pumpkin and melon seeds grow stems that put a fruit
next to them; melons break into slices. Sugar cane grows up to three tall
by the water. Tall grass (sometimes seeds), wild crops, pumpkins, melons,
sugar cane and mushrooms (forests and cave floors) appear in new terrain.
Foods: bread, cookies, baked potato, pumpkin pie, beetroot soup and
mushroom stew (in a bowl, which you keep), milk (a bucket on a cow), and
cake, a block eaten a slice at a time. Sugar and paper from sugar cane, a
pumpkin lantern that glows. Bone meal (from a composter filled with plant
scraps) makes crops shoot up and grass sprout. Sheep and cows love wheat,
pigs carrots, chickens seeds (apples still work). Crafting with a milk
bucket gives the bucket back. The creative palette has tabs now
(Building, Nature, Food, Tools, Items), with pages.

Not quite as planned: plants grow by day and night alike, even in the
dark (the server doesn't know light levels); no ferns yet, and no
trampling of farmland.

What was planned:

- Hoe (wood, stone, iron, diamond): turns dirt and grass into farmland.
  Farmland stays wet within 4 blocks of water; dry farmland grows slowly.
- Tall grass and ferns on the ground; breaking them sometimes gives
  wheat seeds.
- Crops that grow in stages on farmland (faster when lit and wet), and
  pop out their harvest when broken: wheat (seeds → wheat and more
  seeds), carrots, potatoes, beetroots.
- Pumpkins and melons: their seeds grow a stem that puts a fruit next to
  it. Melon slices; a carved pumpkin lantern.
- Sugar cane grows on sand or dirt next to water, up to three tall.
- Food: bread, baked potato, pumpkin pie, cookies, cake (a block you eat
  slice by slice), beetroot soup and mushroom stew (in a bowl).
- Mushrooms (red and brown) in dark places.
- Milk: a bucket on a cow. Sugar from sugar cane, paper too.
- Each animal gets its own favourite food: sheep and cows wheat, pigs
  carrots, chickens seeds (apples still work as a treat).
- A composter that turns plant scraps into bone meal, which makes crops
  grow at once.

### 18. Colours ✅ done

Eight meadow flowers (dandelion, poppy, cornflower, daisy, pink and orange
tulips, allium, forget-me-not) grow in patches. Sixteen dyes come from
flowers, coal (black), bone meal (white), brown mushrooms, beetroots and
cactus (smelted, green), and mix into the rest (red and white make pink,
blue and green cyan…). Dyed wool in every colour (white wool plus a dye);
dyeing a sheep turns its wool that colour, and shearing gives coloured
wool. Most wild sheep are white, some grey, black or brown, and lambs take
after a parent (saved with kept animals). Beds, carpets, stained glass
(see-through) and clay tiles in every colour.

Not quite as planned: no stained glass panes yet (panes come with
milestone 21) and no "candy bricks". White wool and planks now make a
white bed; the red bed is made from red wool.

What was planned:

- Flowers in the meadows (several kinds, original looks), and dyes from
  them (16 colours), plus a few from other things (ink from coal, white
  from bone meal).
- Dyed wool in every colour (and dyeing a sheep so it grows coloured
  wool), coloured beds, carpets, stained glass and glass panes, coloured
  clay tiles (fired clay), coloured "candy bricks" (our own colourful
  building block).

### 19. Trees and stone ✅ done

Five new woods (birch, pine, jungle, cherry and dark) next to oak, each
with logs, planks, leaves, a sapling, stairs, a slab, a fence, a fence
gate, a door and a trapdoor. Every wood has its own tree shape (tall
jungle trees, cone-shaped pines, wide cherry blossom), used both by new
terrain (pines in the snow and mountains, mixed forests, cherry groves on
the plains) and by saplings, which leaves now drop and which grow into a
tree after a while (or with bone meal). Recipes that just need wood take
any planks, even mixed. Boats in every wood: put one on water, get in,
row much faster than swimming, Shift to get out, hit it to pick it up.
New stone: smooth stone, stone bricks (mossy and cracked), mossy
cobblestone, sandstone (smooth and cut; under deserts), granite, diorite
and andesite (patches underground, and polished), deep slate down below
(cobbled, polished, bricks), walls, stairs and slabs for them, and
obsidian where water meets still lava. Snowy lakes freeze over, snow lies
on snowy ground in layers, snowballs make snow blocks, and nine ice make
packed ice.

Not quite as planned: no jungle biome (jungle trees grow in patches of
forest); boats aren't saved with the world; mossy blocks are made with
leaves; ice isn't slippery.

What was planned:

- More trees with their own wood: birch, pine, jungle, cherry-blossom and
  a dark wood, each with logs, planks, leaves, saplings, stairs, slabs,
  fences, gates, doors and trapdoors. Saplings grow into trees; leaves drop
  saplings.
- Boats to row across lakes.
- Stone kinds: smooth stone, stone bricks (mossy and cracked too), mossy
  cobblestone, sandstone (and smooth, cut), granite, diorite and andesite
  (and polished), deep slate further down, walls for the stone kinds,
  ice, packed ice, snow layers, obsidian (from water on still lava).

### 20. Tools and adventure ✅ done

Fishing rods (cast into water, a fish bites after a few seconds and is
caught straight away; now and then string, a feather or even a saddle),
fish and cooked fish. A bow and arrows (flint from gravel, feathers from
chickens, string from wool) for targets: hit one and there's confetti and
a cheer, and survival archers get their arrows back. Armour in wool,
iron, gold and diamond, worn in four slots next to the crafting grid,
softens monsters' bonks (up to four fifths) and shows on your avatar for
everyone else. A compass points to your bed (or the start), a clock tells
the time of day, a map shows the land around you, a spyglass zooms in.
Rope leads animals around; a saddle on a pig lets you ride it (Shift to
get off). A campfire cooks food you put on it.

Not quite as planned: no name tags and no flint and steel yet; armour
doesn't wear out; maps show what's loaded around you rather than
remembering where you've been. Also fixed: the server was announcing the
weather every tick.

What was planned:

- Fishing rod and fish (and cooked fish).
- Bow and arrows for targets: a target block that rings when hit. (Kept
  gentle: arrows don't hurt animals.)
- Armour in leather, iron, gold and diamond: monsters' bonks hurt less.
- Compass (points to your bed or spawn), clock, maps you fill in by
  walking, a spyglass.
- Rope (lead animals around), name tags, a saddle to ride pigs.
- Flint and steel for a campfire; a campfire that cooks food.

### 21. Decoration and storage ✅ done

Flower pots (plant a flower, sapling or mushroom in one), eight paintings
of our own (sunset, mountain, sea, flower, cat, house, rainbow, starry
night; the wool colour in the recipe picks which), item frames that show
whatever you put in them (saved with the world), banners in every
colour, standing or on walls, lanterns, candles and chains, glass panes
(plain and stained) and iron bars that join up like fences, bookshelves,
books and a lectern, barrels that hold things like chests, and scaffolding
to climb.

Not quite as planned: banners are plain colours (no patterns yet); there's
no writable book and no double chest.

What was planned:

- Flower pots, paintings (our own pictures), item frames, banners with
  patterns, carpets, lanterns, candles, glass panes, iron bars, chains.
- Bookshelves, books and a writable book, a lectern.
- Barrels, and a bigger double chest.
- Ladders and scaffolding for building tall things.

### 22. Circuits and transport

- A power source with an original name and look (a glowing "spark" ore
  and dust), wires, switches (levers), buttons, pressure plates, powered
  lamps, doors and trapdoors that open with power, repeaters, pistons
  (normal and sticky), a dispenser, a note block.
- Mine carts, rails, powered rails.

### 23. More creatures

- Animals: horses and donkeys (rideable), rabbits, foxes, cats and dogs
  you can befriend, fish in the water, frogs, bees with hives and honey.
- A few more gentle monsters at night, all original.

### 24. Our own magic

Minecraft's magic (enchanting tables, other dimensions, its special
gems and metals, its creatures) is its own invention, so we don't copy
it. Instead, our own version if kids want it: a "wish table" that gives
tools a sparkle (faster, longer-lasting), potions brewed from plants
(jump higher, glow, swim faster), and maybe a dreamy sky world reached
through a cloud portal.

**Naming rule for all of the above:** everyday things keep their everyday
names (wheat, bucket, boat, compass). Inventions specific to another game
get new names and art, never copied ones.

### Later ideas

- Villages and other structures in world generation.
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
- ~~Sunset sky looks muddy brown~~: fixed (the warm glow is now added on
  top of the sky colour).
- The sun "drawn as a clipped triangle" turned out to be the sun setting
  behind far hills that are fully fogged to the sky colour. Correct, but
  it looks odd; a sky shader that draws the sun and fades distant terrain
  differently would fix it.
