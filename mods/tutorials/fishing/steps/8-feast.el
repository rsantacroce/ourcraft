;; Fishing Fun: step 8, the whole mod.
;;
;; Fish of many kinds, places to find them, better rods, a fish market,
;; cooking, a feast and a fishing journal.

;; ---------------------------------------------------------------
;; Pictures
;; ---------------------------------------------------------------

;; Every fish has the same shape. Each letter is a colour:
;; b = body, w = belly, e = eye, t = tail. A dot is see-through.
(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])

(def boot-shape
  ["..bbb..."
   "..bbb..."
   "..bbb..."
   "..bbbbb."
   "..bbbbbb"
   "..ssssss"])

(def coin-shape
  ["..yyyy.."
   ".yyoyyy."
   "yyoyyyyy"
   "yyoyyyyy"
   "yyyyyyoy"
   ".yyyyoy."
   "..yyyy.."])

(def rod-shape
  [".......s"
   "......ss"
   ".....r.s"
   "....r..s"
   "...r...s"
   "..r....h"
   ".r......"
   "r......."])

;; ---------------------------------------------------------------
;; The fish
;; ---------------------------------------------------------------

;; :weight says how often it bites: 10 is ten times as often as 1.
;; :price is coins at the market (cooked fish sell for double).
(def fish-list
  [{:id "perch"   :display "Puddle Perch"  :body [240 170 60]  :belly [255 230 150] :place "pond"   :weight 10 :price 2}
   {:id "carp"    :display "Bubble Carp"   :body [150 120 200] :belly [220 200 255] :place "pond"   :weight 6  :price 3}
   {:id "tuna"    :display "Big Blue Tuna" :body [40 80 180]   :belly [200 220 240] :place "deep"   :weight 6  :price 5}
   {:id "minnow"  :display "Glow Minnow"   :body [120 255 200] :belly [230 255 240] :place "cave"   :weight 6  :price 6}
   {:id "cod"     :display "Frosty Cod"    :body [180 220 255] :belly [255 255 255] :place "snow"   :weight 6  :price 6}
   {:id "star"    :display "Star Trout"    :body [40 40 90]    :belly [255 240 120] :place "night"  :weight 4  :price 8}
   {:id "rainbow" :display "Rainbow Fish"  :body [255 80 160]  :belly [120 220 255] :place "golden" :weight 1  :price 50}])

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

(register-item {:name "fish:old_boot" :display "Old Boot"
                :picture boot-shape :colors {:b [110 70 40] :s [60 40 20]}})

(register-item {:name "fish:coin" :display "Fish Coin"
                :picture coin-shape :colors {:y [255 210 60] :o [255 250 200]}})

;; ---------------------------------------------------------------
;; Rods (an item whose name ends in _rod casts a line)
;; ---------------------------------------------------------------

(register-item {:name "fish:lucky_rod" :display "Lucky Rod" :stack 1
                :picture rod-shape :colors {:r [120 80 40] :s [230 230 230] :h [80 220 80]}})
(register-item {:name "fish:golden_rod" :display "Golden Rod" :stack 1
                :picture rod-shape :colors {:r [255 200 40] :s [255 255 200] :h [255 120 40]}})

;; A lucky rod: like a fishing rod, with a gold ingot on the line.
(register-recipe {:pattern ["  s" " st" "s g"]
                  :key {:s "core:stick" :t "core:string" :g "core:gold_ingot"}
                  :result "fish:lucky_rod"})

;; A golden rod: made of gold. Only a golden rod catches the Rainbow Fish.
(register-recipe {:pattern ["  g" " gt" "g t"]
                  :key {:g "core:gold_ingot" :t "core:string"}
                  :result "fish:golden_rod"})

;; ---------------------------------------------------------------
;; The feast
;; ---------------------------------------------------------------

(register-item {:name "fish:sandwich" :display "Fish Sandwich" :food 12 :heals 4
                :picture ["........"
                          ".bbbbbb."
                          "gggggggg"
                          ".ffffff."
                          "bbbbbbbb"
                          ".bbbbbb."]
                :colors {:b [220 170 90] :g [90 200 70] :f [200 120 60]}})
(register-recipe {:ingredients ["core:bread" "fish:cooked_perch"] :result "fish:sandwich"})

(register-item {:name "fish:feast" :display "Fisher's Feast" :food 20 :heals 20
                :picture ["..wwww.."
                          ".wyyyyw."
                          "wyoyyoyw"
                          "wyyyyyyw"
                          ".wwwwww."]
                :colors {:w [240 240 240] :y [250 200 90] :o [230 110 50]}})
(register-recipe {:ingredients ["fish:cooked_tuna" "fish:cooked_minnow" "fish:cooked_cod"
                                "fish:cooked_star" "core:bread"]
                  :result "fish:feast"})

;; The market's counter block.
(register-block {:name "fish:market" :display "Fish Market"
                 :picture fish-shape
                 :colors {:b [255 255 255] :w [200 230 255] :e [20 20 20] :t [255 255 255]}
                 :color [40 120 200]})

;; ---------------------------------------------------------------
;; Where are you fishing?
;; ---------------------------------------------------------------

(def water (block-id "core:water"))

;; Rock over your head: a cave.
(defn in-a-cave? (x y z)
  (let (top (ground-height x z))
    (and top (> top (+ y 3)))))

;; Four blocks of water under the bobber: deep water.
(defn deep? (x y z)
  (every? (fn (d) (= (get-block x (- y d) z) water)) [1 2 3 4]))

;; Snow or ice close by.
(def cold-blocks [(block-id "core:snow") (block-id "core:snow_block")
                  (block-id "core:ice") (block-id "core:packed_ice")])
(defn cold? (b) (some? (fn (c) (= b c)) cold-blocks))
(defn snowy? (x y z)
  (some? (fn (d)
           (or (cold? (get-block (+ x (nth d 0)) y (+ z (nth d 1))))
               (cold? (get-block (+ x (nth d 0)) (+ y 1) (+ z (nth d 1))))))
         [[1 0] [-1 0] [0 1] [0 -1] [2 0] [-2 0] [0 2] [0 -2] [1 1] [-1 -1] [1 -1] [-1 1]]))

(defn night? ()
  (let (t (time-of-day)) (or (< t 0.23) (> t 0.77))))

(defn fishing-place (x y z)
  (cond (in-a-cave? x y z) "cave"
        (snowy? x y z)     "snow"
        (deep? x y z)      "deep"
        true               "pond"))

;; ---------------------------------------------------------------
;; What bites?
;; ---------------------------------------------------------------

;; One of the fish, picked by weight.
(defn weighted-pick (things)
  (let (total (sum (map (fn (f) (dict-get f :weight)) things)))
    (loop (left things roll (random total))
      (let (f (head left) w (dict-get f :weight))
        (if (or (< roll w) (empty? (tail left)))
            f
            (recur (tail left) (- roll w)))))))

(defn choose-fish (place rod)
  (let (lucky (= rod "fish:lucky_rod")
        golden (= rod "fish:golden_rod")
        here (filter (fn (f)
                       (let (p (dict-get f :place))
                         (or (= p place)
                             (and (= p "night") (night?))
                             (and (= p "golden") golden))))
                     fish-list))
    ;; A lucky rod makes rare fish (weight under 5) three times as likely.
    (weighted-pick
      (map (fn (f)
             (if (and lucky (< (dict-get f :weight) 5))
                 (dict-set f :weight (* 3 (dict-get f :weight)))
                 f))
           here))))

;; ---------------------------------------------------------------
;; Your fishing journal (kept with the world)
;; ---------------------------------------------------------------

(def journals (or (load-data "journals") {}))

;; A player's name, which stays the same every time they play.
(defn name-of (player)
  (dict-get (head (filter (fn (p) (= (dict-get p :id) player)) (players))) :name))

(defn journal (player)
  (or (dict-get journals (name-of player)) []))

(defn remember-catch (player f)
  (let (seen (journal player) id (dict-get f :id))
    (unless (some? (fn (s) (= s id)) seen)
      (set! journals (dict-set journals (name-of player) (cons id seen)))
      ;; Remember it next time the world opens.
      (save-data "journals" journals)
      (show-title (str "New fish: " (dict-get f :display) "!") 2 player)
      (confetti-on player))))

(defn confetti-on (player)
  (let (p (player-pos player))
    (confetti (nth p 0) (+ (nth p 1) 2) (nth p 2))))

(defn update-hud (player)
  (set-hud player
           [(str "Fish Coins: " (count-item player "fish:coin"))
            (str "Fish found: " (length (journal player)) " of " (length fish-list))]))

;; ---------------------------------------------------------------
;; A bite!
;; ---------------------------------------------------------------

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "You fished up an Old Boot. Yuck!")
          "fish:old_boot")
      (let (f (choose-fish (fishing-place x y z) rod))
        (tell player (str "You caught a " (dict-get f :display) "!"))
        (remember-catch player f)
        (update-hud player)
        (fish-item f))))

;; ---------------------------------------------------------------
;; The fish market
;; ---------------------------------------------------------------

;; /market builds a little stall next to you.
(defn build-market (player)
  (let (p (player-pos player)
        x (+ (nth p 0) 2) y (nth p 1) z (nth p 2))
    (fill x (- y 1) z (+ x 2) (- y 1) (+ z 2) "core:planks")
    (fill x y z (+ x 2) (+ y 3) (+ z 2) "core:air")
    (set-block (+ x 1) y (+ z 1) "fish:market")
    (for-each c [[0 0] [2 0] [0 2] [2 2]]
      (fill (+ x (nth c 0)) y (+ z (nth c 1)) (+ x (nth c 0)) (+ y 2) (+ z (nth c 1)) "core:fence"))
    (fill x (+ y 3) z (+ x 2) (+ y 3) (+ z 2) "core:wool")
    (set-block (+ x 1) (+ y 2) (+ z 1) "core:lantern")
    (confetti (+ x 1) (+ y 1) (+ z 1))
    (tell player "The Fish Market is open! Stand next to it and type /sell")))

;; Is there a market counter close to the player?
(def market (block-id "fish:market"))
(defn near-market? (player)
  (let (p (player-pos player))
    (some? (fn (dx)
             (some? (fn (dz)
                      (some? (fn (dy)
                               (= (get-block (+ (nth p 0) dx) (+ (nth p 1) dy) (+ (nth p 2) dz)) market))
                             [-1 0 1]))
                    [-3 -2 -1 0 1 2 3]))
           [-3 -2 -1 0 1 2 3])))

;; Sell every fish you carry.
(defn sell-everything (player)
  (let (coins 0)
    (for-each f fish-list
      (let (raw (take-item player (fish-item f) 999)
            cooked (take-item player (cooked-item f) 999))
        (set! coins (+ coins (* raw (dict-get f :price))
                             (* cooked 2 (dict-get f :price))))))
    (set! coins (+ coins (take-item player "fish:old_boot" 999)))
    (if (= coins 0)
        (tell player "You have no fish to sell. Go fishing!")
        (do (give-item player "fish:coin" coins)
            (sound "pop")
            (tell player (str "You sold your fish for " coins " Fish Coins!"))))
    (update-hud player)))

;; Things the market sells: [what price name].
(def shop
  [["fish:golden_rod" 30 "Golden Rod (catches the Rainbow Fish!)"]
   ["core:fishing_rod" 3 "Fishing Rod"]
   ["core:bread" 2 "Bread"]
   ["core:lantern" 4 "Lantern"]])

(defn show-shop (player)
  (tell player "The market sells (type /buy and the number):")
  (for-each i (range (length shop))
    (let (thing (nth shop i))
      (tell player (str "  " (+ i 1) ". " (nth thing 2) " - " (nth thing 1) " coins")))))

(defn buy (player number)
  (let (thing (if (> number 0) (nth shop (- number 1)) nil))
    (cond (nil? thing)
          (tell player "There's nothing with that number. Type /shop")
          (< (count-item player "fish:coin") (nth thing 1))
          (tell player (str "You need " (nth thing 1) " Fish Coins for that."))
          true
          (do (take-item player "fish:coin" (nth thing 1))
              (give-item player (nth thing 0))
              (sound "pop")
              (tell player (str "You bought: " (nth thing 2)))
              (update-hud player)))))

;; ---------------------------------------------------------------
;; Chat commands
;; ---------------------------------------------------------------

;; "2" becomes 2. Anything that isn't a number becomes 0.
(defn as-number (word)
  (if (and word (str-matches word "^[0-9]+$")) (->number word) 0))

(defn on-join (player name)
  (tell player (str "Welcome, " name "! Fishing Fun is on. Type /fishing for help."))
  (update-hud player))

(defn on-chat (player text)
  (let (words (str-split (str-trim text) " ")
        command (head words))
    (cond
      (= command "/fishing")
      (do (tell player "Fish with a fishing rod: point at water and right-click.")
          (tell player "/market builds a market, /sell sells your fish,")
          (tell player "/shop shows what you can buy, /journal shows your fish.")
          true)
      (= command "/market") (do (build-market player) true)
      (= command "/sell")
      (do (if (near-market? player)
              (sell-everything player)
              (tell player "Stand next to a Fish Market to sell. (/market builds one)"))
          true)
      (= command "/shop") (do (show-shop player) true)
      (= command "/buy")
      (do (if (near-market? player)
              (buy player (as-number (nth words 1)))
              (tell player "Stand next to a Fish Market to buy."))
          true)
      (= command "/journal")
      (do (tell player (str "You have found " (length (journal player)) " of "
                            (length fish-list) " fish:"))
          (for-each f fish-list
            (when (some? (fn (s) (= s (dict-get f :id))) (journal player))
              (tell player (str "  " (dict-get f :display)))))
          true)
      true nil)))
