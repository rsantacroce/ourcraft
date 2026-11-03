;; Fishing Fun, step 5: places to fish.

;; ---------------------------------------------------------------
;; Pictures
;; ---------------------------------------------------------------

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

;; ---------------------------------------------------------------
;; The fish
;; ---------------------------------------------------------------

;; :weight says how often it bites: 10 is ten times as often as 1.
(def fish-list
  [{:id "perch"   :display "Puddle Perch"  :body [240 170 60]  :belly [255 230 150] :place "pond"   :weight 10}
   {:id "carp"    :display "Bubble Carp"   :body [150 120 200] :belly [220 200 255] :place "pond"   :weight 6}
   {:id "tuna"    :display "Big Blue Tuna" :body [40 80 180]   :belly [200 220 240] :place "deep"   :weight 6}
   {:id "minnow"  :display "Glow Minnow"   :body [120 255 200] :belly [230 255 240] :place "cave"   :weight 6}
   {:id "cod"     :display "Frosty Cod"    :body [180 220 255] :belly [255 255 255] :place "snow"   :weight 6}
   {:id "star"    :display "Star Trout"    :body [40 40 90]    :belly [255 240 120] :place "night"  :weight 4}
   {:id "rainbow" :display "Rainbow Fish"  :body [255 80 160]  :belly [120 220 255] :place "deep"   :weight 1}])

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

(defn choose-fish (place)
  (let (here (filter (fn (f)
                       (let (p (dict-get f :place))
                         (or (= p place)
                             (and (= p "night") (night?)))))
                     fish-list))
    (weighted-pick here)))

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "You fished up an Old Boot. Yuck!")
          "fish:old_boot")
      (let (f (choose-fish (fishing-place x y z)))
        (tell player (str "You caught a " (dict-get f :display) "!"))
        (fish-item f))))

(defn on-join (player name)
  (tell player (str "Hello, " name "! Fishing Fun is on.")))

(defn on-chat (player text)
  (when (= text "/fishing")
    (tell player "Fish in ponds, deep water, caves, snowy places and at night!")
    true))
