;; Pesca Divertida, passo 3: pesque um peixe e depois cozinhe.

(def fish-shape
  ["........"
   "...bbb.t"
   ".bbbbbtt"
   "bebbbbbt"
   "bwwwwbtt"
   ".wwwww.t"
   "........"
   "........"])

;; :cooks-into diz o que a fornalha ou a fogueira faz com ele.
(register-item {:name "fish:perch"
                :display "Perca-da-Poça"
                :picture fish-shape
                :colors {:b [240 170 60] :w [255 230 150] :e [20 20 20] :t [240 170 60]}
                :food 2
                :cooks-into "fish:cooked_perch"})

;; O mesmo desenho, só que marrom: está cozido!
(register-item {:name "fish:cooked_perch"
                :display "Filé de Perca-da-Poça"
                :picture fish-shape
                :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                :food 6})

;; Um peixe mordeu a isca! O que isto devolve é o que você pesca.
(defn on-fish-bite (player rod x y z)
  (tell player "Você pescou uma Perca-da-Poça!")
  "fish:perch")

(defn on-join (player name)
  (tell player (str "Olá, " name "! A Pesca Divertida está ligada.")))

(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player "Aponte uma vara de pescar para a água e clique com o botão direito. Depois é só esperar...")
    true))
