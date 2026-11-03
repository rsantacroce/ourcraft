;; Glow Garden: a mod written in EELisp.
;;
;; Put this file in the server's mods folder and it's a mod. It adds two
;; blocks, a /garden command, a welcome, and keeps glow blocks safe from
;; anyone who isn't the gardener.

;; Blocks are registered while the script first runs.
(register-block {:name "garden:glow" :display "Glow Block" :color [255 210 90] :light 15})
(register-block {:name "garden:leafglass" :display "Leaf Glass" :color [80 200 120] :glass true})

(def gardener nil)    ; the first player to use /garden

(defn on-join (player name)
  (tell player (str "Welcome to the Glow Garden, " name "! Type /garden")))

;; /garden: a 5x5 bed of grass with glow blocks in the corners.
(defn garden (player)
  (let (p (player-pos player)
        x (+ (nth p 0) 2) y (- (nth p 1) 1) z (+ (nth p 2) 2))
    (fill x y z (+ x 4) y (+ z 4) "core:grass")
    (for-each c [[0 0] [4 0] [0 4] [4 4]]
      (set-block (+ x (nth c 0)) (+ y 1) (+ z (nth c 1)) "garden:glow"))
    (confetti (+ x 2) (+ y 2) (+ z 2))
    (when (= gardener nil) (set! gardener player))
    (show-title "A garden grows!" 2 player)))

(defn on-chat (player text)
  (when (= (str-trim text) "/garden")
    (garden player)
    true))

;; Only the gardener may break glow blocks.
(defn on-block-change (player x y z old new)
  (if (and (= old (block-id "garden:glow")) (!= player gardener))
    (do (tell player "Only the gardener can pick glow blocks!") :deny)
    nil))
