;; Fishing Fun, step 3: catch a fish, then cook it.

(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])

;; :cooks-into says what a furnace or a campfire makes of it.
(register-item {:name "fish:perch"
                :display "Puddle Perch"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2
                :cooks-into "fish:cooked_perch"})

;; The same shape in brown: it's cooked!
(register-item {:name "fish:cooked_perch"
                :display "Cooked Puddle Perch"
                :picture fish-shape
                :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                :food 6})

;; A fish bites! Whatever this gives back is what you catch.
(defn on-fish-bite (player rod x y z)
  (tell player "You caught a Puddle Perch!")
  "fish:perch")

(defn on-join (player name)
  (tell player (str "Hello, " name "! Fishing Fun is on.")))

(defn on-chat (player text)
  (when (= text "/fishing")
    (tell player "Point a fishing rod at water and right-click. Then wait...")
    true))
