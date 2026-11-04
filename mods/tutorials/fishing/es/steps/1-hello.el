;; Pesca Divertida, paso 1: saluda.
;;
;; Una línea que empieza por ; es una nota para personas. El juego se la salta.

;; Cuando entra un jugador, salúdalo.
(defn on-join (player name)
  (tell player (str "¡Hola, " name "! Pesca Divertida está en marcha.")))

;; Cuando un jugador escribe en el chat.
(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player "¡Pronto pescarás peces aquí!")
    true))
