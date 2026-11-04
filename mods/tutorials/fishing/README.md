# Fishing Fun: make your own fishing mod

In this tutorial you'll make a **mod** for Ourcraft. A mod is a file that
adds new things to the game. Yours will add:

- 🐟 fish you draw yourself, from the Puddle Perch to the Rainbow Fish
- 🏞️ places to fish: ponds, deep water, caves, snowy places, and night time
- 🎣 better fishing rods: a Lucky Rod and a Golden Rod
- 🪙 a fish market where you sell your fish for coins and buy things
- 🔥 cooking, a Fish Sandwich, and a big Fisher's Feast
- 📖 a fishing journal that counts the fish you've found

You'll write it in **EELisp**, a computer language. You don't need to know
it yet: you'll learn it on the way, one small step at a time. There are 8
steps. Each one is a complete mod that works, so you can stop after any of
them and play.

All the finished files are in the [`steps`](steps) folder, if you get
stuck.

**Other languages:** the step files are also in Brazilian Portuguese
([`pt-BR/steps`](pt-BR/steps)) and Spanish from Spain ([`es/steps`](es/steps)),
with their comments and in-game messages translated. The whole tutorial is
on the website in [English](https://rsantacroce.github.io/ourcraft-site/fishing.html),
[português](https://rsantacroce.github.io/ourcraft-site/fishing-pt-br.html) and
[español](https://rsantacroce.github.io/ourcraft-site/fishing-es.html).

---

## For a grown-up: getting ready (5 minutes)

1. **A text editor.** Any editor that saves plain text works. Good ones are
   [Visual Studio Code](https://code.visualstudio.com) (free, colours the
   brackets so they're easy to match) or Notepad on Windows. On a Mac,
   TextEdit works if you choose **Format → Make Plain Text** first.
2. **The mods folder.** Start Ourcraft, click **Play**, then the **Mods**
   button next to any world. The screen says where the mods folder is. On
   a Mac it's `~/Library/Application Support/Ourcraft/mods`. Make a file
   there called **`fishing.el`**. That name stays the same for the whole
   tutorial: each step, you change what's inside it.
3. **A world to try it in.** Make a new **Creative** world. In Creative, the
   inventory (press **E**) has every item, including the fishing rod and
   everything your mod adds. Later, for crafting and selling, a
   **Survival** world is more fun.
4. **Switch the mod on.** In the world list, click **Mods** next to the
   world, and click **Fishing** until it says **on**. New mods are off
   until you switch them on, so a mod only goes into the worlds you choose.

**How to try a change:** save the file, then in the game go back to the
title screen (**Esc → Save and quit to title**) and **Play** the world again. The
game reads your mod each time a world opens.

---

## Step 1: Say hello

Put this in `fishing.el`:

```lisp
;; Fishing Fun, step 1: say hello.
;;
;; A line that starts with ; is a note for people. The game skips it.

;; When a player joins, tell them hello.
(defn on-join (player name)
  (tell player (str "Hello, " name "! Fishing Fun is on.")))

;; When a player types in the chat.
(defn on-chat (player text)
  (when (= text "/fishing")
    (tell player "Soon you'll catch fish here!")
    true))
```

**Try it:** play the world. You'll see the hello message. Press **/** to
start a command (or **Enter** to chat), type `/fishing`, and press
**Enter**.

### What's going on?

In EELisp, everything is in **brackets**. The first word in the brackets
says what to do, and the rest are the things to do it with:

```lisp
(tell player "Soon you'll catch fish here!")
```

means "**tell** this **player** this message".

`defn` **defines** something the game can use. `on-join` is special: the
game uses it every time a player joins. `on-chat` is used every time
someone types in the chat.

`(str "Hello, " name "!")` sticks words together into one message.
`(when (= text "/fishing") ...)` means "when the text is `/fishing`, do
this". The `true` at the end tells the game "this was my command", so
nobody else sees it in the chat.

> **Make it yours:** change the hello message. Add a second command,
> `/hi`, that says something funny.

---

## Step 2: Draw a fish

Now let's make a fish you can hold. You **draw it with letters**! Each
letter is a colour, and a dot is see-through. Squint at it and you'll see
a fish swimming to the left:

```lisp
(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])
```

`def` gives something a name. Now `fish-shape` means this picture.

Then you tell the game about your new item:

```lisp
(register-item {:name "fish:perch"
                :display "Puddle Perch"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2})
```

The curly brackets `{ }` hold a list of **settings**. Each setting has a
name that starts with `:` and then its value:

- `:name` is the item's secret name inside the game. It always starts
  with `fish:` (the name of your mod) and has no spaces.
- `:display` is the name players see.
- `:colors` says what colour each letter is. A colour is three numbers in
  square brackets: **[red green blue]**, each from 0 to 255. `[255 0 0]`
  is bright red, `[0 0 0]` is black, `[255 255 255]` is white.
- `:food 2` means you can eat it, and it fills 2 points of hunger.

Change `on-chat` so `/fishing` gives you one:

```lisp
(defn on-chat (player text)
  (when (= text "/fishing")
    (give-item player "fish:perch")
    (tell player "Here's a Puddle Perch!")
    true))
```

The whole file is [`steps/2-draw-a-fish.el`](steps/2-draw-a-fish.el).

**Try it:** type `/fishing` and look in your hotbar. That's your fish!

> **Make it yours:** change the colours. Make a green fish, or a purple
> one with a yellow belly. Draw a new shape! The picture can be up to 16
> letters wide and 16 rows tall.

---

## Step 3: Catch it, then cook it

Ourcraft already has fishing: hold a **Fishing Rod**, point at water,
right-click, and wait for a bite. Your mod can decide **what** bites. Add
this:

```lisp
(defn on-fish-bite (player rod x y z)
  (tell player "You caught a Puddle Perch!")
  "fish:perch")
```

The game uses `on-fish-bite` when a fish bites. The last thing in it,
`"fish:perch"`, is what the player catches. (`x y z` say where the
bobber is. You'll use them in step 5.)

Now let's cook it. Add `:cooks-into` to the perch, and make a cooked one
in brown:

```lisp
(register-item {:name "fish:perch"
                :display "Puddle Perch"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2
                :cooks-into "fish:cooked_perch"})

(register-item {:name "fish:cooked_perch"
                :display "Cooked Puddle Perch"
                :picture fish-shape
                :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                :food 6})
```

The whole file is [`steps/3-catch-and-cook.el`](steps/3-catch-and-cook.el).

**Try it:** take a Fishing Rod from the inventory (**E**). Find water,
point at it and right-click. Wait a few seconds... a Puddle Perch! Put
it in a **furnace**, or hold it and right-click a **campfire**, to cook
it. Cooked fish fills you up much more.

> **Make it yours:** make the cooked fish fill even more hunger. What's
> the most it can be? (It's 20, a whole hunger bar.)

---

## Step 4: Lots of fish

One kind of fish gets boring. Let's make a **list** of fish. Each fish
has its settings in `{ }`, and the whole list is in `[ ]`:

```lisp
(def fish-list
  [{:id "perch"   :display "Puddle Perch"  :body [240 170 60]  :belly [255 230 150] :weight 10}
   {:id "carp"    :display "Bubble Carp"   :body [150 120 200] :belly [220 200 255] :weight 6}
   {:id "tuna"    :display "Big Blue Tuna" :body [40 80 180]   :belly [200 220 240] :weight 3}
   {:id "rainbow" :display "Rainbow Fish"  :body [255 80 160]  :belly [120 220 255] :weight 1}])
```

`:weight` says how often a fish bites. A fish with weight 10 bites ten
times as often as a fish with weight 1. So the Rainbow Fish is **rare**!

Instead of writing `register-item` for every fish, a **loop** does it for
each fish in the list:

```lisp
(defn fish-item (f) (str "fish:" (dict-get f :id)))
(defn cooked-item (f) (str "fish:cooked_" (dict-get f :id)))

(for-each f fish-list
  (register-item {:name (fish-item f)
                  :display (dict-get f :display)
                  :picture fish-shape
                  :colors {:b (dict-get f :body) :w (dict-get f :belly)
                           :e [20 20 20] :t (dict-get f :body)}
                  :food 2
                  :cooks-into (cooked-item f)})
  (register-item {:name (cooked-item f)
                  :display (str "Cooked " (dict-get f :display))
                  :picture fish-shape
                  :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                  :food 6}))
```

`for-each f fish-list` means "for each fish in the list, call it `f`,
and do this". `(dict-get f :body)` reads the `:body` setting of that fish.
`fish-item` is a little helper you made: give it a fish, and it makes its
name, like `"fish:carp"`.

Now the bite picks a fish. `weighted-pick` is a helper in the step file
that picks one, using the weights. And there's a small chance of
something funny:

```lisp
(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "You fished up an Old Boot. Yuck!")
          "fish:old_boot")
      (let (f (weighted-pick fish-list))
        (tell player (str "You caught a " (dict-get f :display) "!"))
        (fish-item f))))
```

- `(chance 8)` is true 8 times out of 100.
- `(if test this that)` does **this** if the test is true, or **that**
  if it isn't.
- `(let (f ...) ...)` gives a name to something for a little while: here,
  `f` is the fish that bit.

The whole file, with the boot and `weighted-pick`, is
[`steps/4-many-fish.el`](steps/4-many-fish.el).

> **Make it yours:** add your own fish to the list: just one more line!
> Give it a name, colours and a weight. Then go and catch it.

---

## Step 5: Places to fish

Real fish live in different places. Let's give each fish a `:place`:

```lisp
{:id "minnow" :display "Glow Minnow" :body [120 255 200] :belly [230 255 240] :place "cave" :weight 6}
```

The places are `"pond"`, `"deep"`, `"cave"`, `"snow"` and `"night"`. But
how does the mod know where the bobber is? It **looks at the blocks**
around it! `(get-block x y z)` tells you which block is at a spot:

```lisp
;; Four blocks of water under the bobber: deep water.
(defn deep? (x y z)
  (every? (fn (d) (= (get-block x (- y d) z) water)) [1 2 3 4]))

;; Rock over your head: a cave.
(defn in-a-cave? (x y z)
  (let (top (ground-height x z))
    (and top (> top (+ y 3)))))
```

- `y` goes **up**. `(- y 1)` is one block below, `(+ y 1)` one block above.
- `(ground-height x z)` is how high the ground is there. If there's rock
  well above the water, you're in a cave!
- A name ending in `?`, like `deep?`, is a question. The answer is
  `true` or `false`.

Then one helper puts it all together:

```lisp
(defn fishing-place (x y z)
  (cond (in-a-cave? x y z) "cave"
        (snowy? x y z)     "snow"
        (deep? x y z)      "deep"
        true               "pond"))
```

`cond` asks the questions in order and gives the first answer that's
true. The last one, `true`, catches everything else: a pond.

The whole file, with `snowy?` and `night?`, is
[`steps/5-fishing-places.el`](steps/5-fishing-places.el).

**Try it:** fish in a pond, then in the sea, then underground, then at
night. (Type `/time night` to make it night straight away.)

> **Make it yours:** invent a new place! Maybe a "jungle" place, near
> `core:leaves` blocks? Or a "high" place, when `y` is above 100?

---

## Step 6: Better rods

Any item whose name ends in **`_rod`** works as a fishing rod. Let's
draw a rod and make two:

```lisp
(register-item {:name "fish:lucky_rod" :display "Lucky Rod" :stack 1
                :picture rod-shape :colors {:r [120 80 40] :s [230 230 230] :h [80 220 80]}})
```

`:stack 1` means you can only hold one in each slot, like real tools.

Now a **recipe**, so you can craft it. The pattern is the crafting grid,
3 by 3, and each letter is an item:

```lisp
(register-recipe {:pattern ["  s"
                            " st"
                            "s g"]
                  :key {:s "core:stick" :t "core:string" :g "core:gold_ingot"}
                  :result "fish:lucky_rod"})
```

The rod tells `on-fish-bite` which rod you used, so rods can be
different:

- The **Lucky Rod** makes rare fish 3 times as likely.
- Only the **Golden Rod** catches the Rainbow Fish.

The whole file is [`steps/6-better-rods.el`](steps/6-better-rods.el).

> **Make it yours:** make a Junk Rod that catches only old boots. Or a
> Sea Rod that only works in deep water.

---

## Step 7: The fish market

Time to get rich! Each fish gets a `:price`, and there's a new item: the
**Fish Coin**. Coins are real items, so they're saved with your world.

`/market` builds a little market stall next to you, with a **Fish Market**
counter in the middle. Standing next to it, you can:

- `/sell`: sell every fish you carry. Cooked fish sell for **double**,
  so it's worth cooking them first!
- `/shop`: see what the market sells.
- `/buy 1`: buy thing number 1 (the Golden Rod, for 30 coins).

Selling uses two new commands:

```lisp
(take-item player "fish:perch" 999)   ; take all your perch; says how many it took
(give-item player "fish:coin" 6)      ; give 6 coins
```

And the shop is just a list, so it's easy to add things to:

```lisp
(def shop
  [["fish:golden_rod" 30 "Golden Rod (catches the Rainbow Fish!)"]
   ["core:fishing_rod" 3 "Fishing Rod"]
   ["core:bread" 2 "Bread"]
   ["core:lantern" 4 "Lantern"]])
```

Your coins show in the corner of the screen, with `set-hud`.

The whole file is [`steps/7-fish-market.el`](steps/7-fish-market.el).

**Try it** in a **Survival** world: catch fish, cook some, build a
market, sell, and save up for the Golden Rod.

> **Make it yours:** sell more things in the shop. Make the Rainbow Fish
> worth 100 coins. Make the stall bigger, or a different colour.

---

## Step 8: A feast, and your fishing journal

Two new recipes use your cooked fish:

```lisp
(register-recipe {:ingredients ["core:bread" "fish:cooked_perch"] :result "fish:sandwich"})
```

`:ingredients` means "put these anywhere in the crafting grid". The
**Fish Sandwich** fills 12 hunger. The **Fisher's Feast** needs four
different cooked fish from four different places, and fills you up all
the way.

The **fishing journal** remembers which fish you've found. The first
time you catch a new kind, there's a big title and confetti! `/journal`
lists them, and the corner of the screen shows "Fish found: 3 of 7".

The finished mod is [`steps/8-feast.el`](steps/8-feast.el). Type
`/fishing` in the game to see all its commands.

> **Make it yours:** give a prize when someone has found every fish:
> 100 coins, or a Golden Rod!

---

## When something goes wrong

Everyone's code goes wrong sometimes, even grown-up programmers. When
your mod has a problem, the game says so in the **chat**, starting with
**(!)**:

- **`The script has a problem: ...`** means the game couldn't read your
  file. It stops at the first problem, so anything after it is missing
  too.
- **`(on-chat …) failed: ...`** means something went wrong while your
  `on-chat` was running. The name tells you where to look.

Things that often go wrong:

| What you see | What to check |
|---|---|
| `unclosed list` or `unclosed dict` | A `(`, `[` or `{` is missing its partner `)`, `]` or `}`. Count them! A good editor colours pairs to help. |
| `expected function, got number` | You wrote a list with round brackets, like `(255 0 0)`. Lists use square ones: `[255 0 0]`. |
| `the picture uses the letter z...` | A letter in your `:picture` isn't in `:colors`. |
| `there's already something with that name` | Two items have the same `:name`. |
| `Undefined symbol: ...` | A misspelled name, or something used before its `def`. |
| `there's no block called ...` | Block names start with `core:`, like `core:stone`. |
| Nothing happens at all | Is the mod switched **on** for this world? Did you save the file, and Play the world again? |

---

## Cheat sheet

**Things the game calls in your mod** (all optional):

| You write | The game uses it when |
|---|---|
| `(defn on-join (player name) ...)` | a player joins |
| `(defn on-chat (player text) ...)` | someone chats (give back `true` if it was your command) |
| `(defn on-fish-bite (player rod x y z) ...)` | a fish bites (give back the item they catch) |
| `(defn on-block-change (player x y z old new) ...)` | someone breaks or places a block (give back `:deny` to stop it) |
| `(defn on-tick (tick) ...)` | 20 times a second |

**Things your mod can do:**

| Command | What it does |
|---|---|
| `(tell player "hi")` / `(chat "hi")` | a message to one player / everyone |
| `(show-title "Wow!" 2 player)` | big text in the middle of the screen for 2 seconds |
| `(set-hud player ["line 1" "line 2"])` | the panel in the corner |
| `(give-item player "fish:perch" 3)` | give items |
| `(take-item player "fish:perch" 3)` | take up to 3 (and say how many it took) |
| `(count-item player "fish:coin")` | how many they have |
| `(register-item {...})` | a new item (only at the top of the file) |
| `(register-block {...})` | a new block (only at the top of the file) |
| `(register-recipe {...})` | a new crafting recipe (only at the top of the file) |
| `(get-block x y z)` / `(set-block x y z "core:stone")` | look at / change one block |
| `(fill x1 y1 z1 x2 y2 z2 "core:glass")` | a whole box of blocks |
| `(player-pos player)` | where they are, as `[x y z]` |
| `(random 6)` / `(chance 25)` / `(pick ["a" "b"])` | a number from 0 to 5 / true 25 times out of 100 / one of them |
| `(confetti x y z)` / `(hearts x y z)` / `(sound "pop")` | a little celebration |
| `(time-of-day)` | 0 is midnight, 0.5 is noon |

**The language:**

| | |
|---|---|
| `(def name value)` | give something a name |
| `(defn name (inputs) ...)` | make a command of your own |
| `(let (a 1 b 2) ...)` | names for a little while |
| `(if test this that)` / `(when test ...)` / `(cond test answer ...)` | choosing |
| `(for-each x list ...)` | do something for each thing in a list |
| `(+ 1 2)` `(- 5 1)` `(* 2 3)` `(/ 10 2)` | maths |
| `(= a b)` `(< a b)` `(> a b)` `(and ...)` `(or ...)` `(not ...)` | questions |
| `(str "a" 1 "b")` | stick words together |
| `[1 2 3]` and `(nth list 0)` | a list, and its first thing (counting starts at 0!) |
| `{:a 1}` and `(dict-get d :a)` | settings, and reading one |

---

## More ideas

- A **fishing contest**: whoever catches the most fish in 3 minutes wins
  (use `on-tick` to count the time).
- **Pet fish**: a fish tank block, where your rarest fish swims.
- **Treasure**: sometimes you fish up a chest key, or a map.
- **Weather**: rain makes the fish bite faster.
- A **giant fish** that takes three bites to catch.

Have fun, and show your friends what you made! 🎣
