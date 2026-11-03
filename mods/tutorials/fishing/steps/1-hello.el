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
