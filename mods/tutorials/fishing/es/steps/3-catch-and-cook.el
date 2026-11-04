;; Pesca Divertida, paso 3: pesca un pez y luego cocínalo.

(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])

;; :cooks-into dice en qué lo convierte un horno o una hoguera.
(register-item {:name "fish:perch"
                :display "Perca del Charco"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2
                :cooks-into "fish:cooked_perch"})

;; La misma forma, en marrón: ¡ya está hecho!
(register-item {:name "fish:cooked_perch"
                :display "Perca del Charco a la brasa"
                :picture fish-shape
                :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                :food 6})

;; ¡Pica un pez! Lo que esto devuelva es lo que pescas.
(defn on-fish-bite (player rod x y z)
  (tell player "¡Has pescado una Perca del Charco!")
  "fish:perch")

(defn on-join (player name)
  (tell player (str "¡Hola, " name "! Pesca Divertida está en marcha.")))

(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player "Apunta al agua con una caña de pescar y haz clic derecho. Luego, espera...")
    true))
