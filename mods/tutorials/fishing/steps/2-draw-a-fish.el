;; Fishing Fun, step 2: draw a fish.

;; A picture made of letters. Each letter is a colour:
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

;; A new item: the Puddle Perch.
(register-item {:name "fish:perch"
                :display "Puddle Perch"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2})

(defn on-join (player name)
  (tell player (str "Hello, " name "! Fishing Fun is on.")))

;; /fishing gives you a fish, to see what it looks like.
(defn on-chat (player text)
  (when (= text "/fishing")
    (give-item player "fish:perch")
    (tell player "Here's a Puddle Perch!")
    true))
