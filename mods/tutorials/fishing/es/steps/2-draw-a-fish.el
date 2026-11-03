;; Pesca Divertida, paso 2: dibuja un pez.

;; Un dibujo hecho con letras. Cada letra es un color:
;; b = cuerpo, w = barriga, e = ojo, t = cola. Un punto es transparente.
(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])

;; Un objeto nuevo: la Perca del Charco.
(register-item {:name "fish:perch"
                :display "Perca del Charco"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2})

(defn on-join (player name)
  (tell player (str "¡Hola, " name "! Pesca Divertida está en marcha.")))

;; /pescar te da un pez, para que veas cómo es.
(defn on-chat (player text)
  (when (= text "/pescar")
    (give-item player "fish:perch")
    (tell player "¡Aquí tienes una Perca del Charco!")
    true))
