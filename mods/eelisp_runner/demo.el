;; A tiny Ourcraft mod in EELisp.
;;
;; The game calls these functions when things happen:
;;   (on-start)              the world is ready
;;   (on-join player name)   someone joined
;;   (on-chat player text)   a chat line; return true if it was your command
;;
;; and the script can call: chat, tell, players, player-pos, block-id,
;; get-block, set-block, fill, show-title, log.

(defn on-join (player name)
  (tell player (str "Hello " name "! This mod is written in EELisp. Try /tower or /wall")))

;; /tower: ten stone blocks going up, two blocks in front of you, one at a time.
(defn tower (player)
  (let (p (player-pos player)
        x (nth p 0) y (nth p 1) z (+ (nth p 2) 2)
        stone (block-id "core:stone"))
    (for-each i (range 0 10)
      (set-block x (+ y i) z stone))
    (tell player "Up goes a tower!")))

;; /wall N: an N-long wall, 3 high, block by block (to see how fast the language is).
(defn wall (player n)
  (let (p (player-pos player)
        x (nth p 0) y (nth p 1) z (+ (nth p 2) 3)
        bricks (block-id "core:bricks"))
    (for-each dx (range 0 n)
      (for-each dy (range 0 3)
        (set-block (+ x dx) (+ y dy) z bricks)))
    (tell player (str "A wall of " (* n 3) " bricks!"))))

;; /bigwall N: the same, but `fill` does the work in one go.
(defn bigwall (player n)
  (let (p (player-pos player)
        x (nth p 0) y (nth p 1) z (+ (nth p 2) 4))
    (tell player (str "Filled " (fill x y z (+ x n -1) (+ y 2) z "core:bricks") " blocks"))))

(defn on-chat (player text)
  (let (words (str-split (str-trim text) " "))
    (cond
      (= (head words) "/tower") (do (tower player) true)
      (= (head words) "/wall") (do (wall player (->number (nth words 1))) true)
      (= (head words) "/bigwall") (do (bigwall player (->number (nth words 1))) true)
      true false)))
