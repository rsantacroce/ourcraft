;; Pesca Divertida, passo 2: desenhe um peixe.

;; Um desenho feito de letras. Cada letra é uma cor:
;; b = corpo, w = barriga, e = olho, t = rabo. Um ponto é transparente.
(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])

;; Um item novo: a Perca-da-Poça.
(register-item {:name "fish:perch"
                :display "Perca-da-Poça"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2})

(defn on-join (player name)
  (tell player (str "Olá, " name "! A Pesca Divertida está ligada.")))

;; /pescar te dá um peixe, para você ver como ele é.
(defn on-chat (player text)
  (when (= text "/pescar")
    (give-item player "fish:perch")
    (tell player "Aqui está uma Perca-da-Poça!")
    true))
