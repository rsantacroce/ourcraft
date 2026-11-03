;; Pesca Divertida, passo 4: um monte de peixes.

;; ---------------------------------------------------------------
;; Desenhos
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
;; Os peixes
;; ---------------------------------------------------------------

;; :weight diz quantas vezes ele morde a isca: 10 morde dez vezes mais que 1.
(def fish-list
  [{:id "perch"   :display "Perca-da-Poça"     :body [240 170 60]  :belly [255 230 150] :weight 10}
   {:id "carp"    :display "Carpa-Bolha"       :body [150 120 200] :belly [220 200 255] :weight 6}
   {:id "tuna"    :display "Atum Azulão"       :body [40 80 180]   :belly [200 220 240] :weight 3}
   {:id "rainbow" :display "Peixe Arco-Íris"   :body [255 80 160]  :belly [120 220 255] :weight 1}])

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
                  :display (str "Filé de " (dict-get f :display))
                  :picture fish-shape
                  :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                  :food 6}))

(register-item {:name "fish:old_boot" :display "Bota Velha"
                :picture boot-shape :colors {:b [110 70 40] :s [60 40 20]}})

;; ---------------------------------------------------------------
;; Quem morde a isca?
;; ---------------------------------------------------------------

;; Um dos peixes, sorteado pelo peso.
(defn weighted-pick (things)
  (let (total (sum (map (fn (f) (dict-get f :weight)) things)))
    (loop (left things roll (random total))
      (let (f (head left) w (dict-get f :weight))
        (if (or (< roll w) (empty? (tail left)))
            f
            (recur (tail left) (- roll w)))))))

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "Você pescou uma Bota Velha. Eca!")
          "fish:old_boot")
      (let (f (weighted-pick fish-list))
        (tell player (str "Você pescou: " (dict-get f :display) "!"))
        (fish-item f))))

(defn on-join (player name)
  (tell player (str "Olá, " name "! A Pesca Divertida está ligada.")))

(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player (str "Existem " (length fish-list) " tipos de peixe. Será que você pesca todos?"))
    true))
