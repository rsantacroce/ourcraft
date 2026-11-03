;; Pesca Divertida, passo 6: varas melhores.

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

(def rod-shape
  [".......s"
   "......ss"
   ".....r.s"
   "....r..s"
   "...r...s"
   "..r....h"
   ".r......"
   "r......."])

;; ---------------------------------------------------------------
;; Os peixes
;; ---------------------------------------------------------------

;; :weight diz quantas vezes ele morde a isca: 10 morde dez vezes mais que 1.
(def fish-list
  [{:id "perch"   :display "Perca-da-Poça"     :body [240 170 60]  :belly [255 230 150] :place "pond"   :weight 10}
   {:id "carp"    :display "Carpa-Bolha"       :body [150 120 200] :belly [220 200 255] :place "pond"   :weight 6}
   {:id "tuna"    :display "Atum Azulão"       :body [40 80 180]   :belly [200 220 240] :place "deep"   :weight 6}
   {:id "minnow"  :display "Lambari Brilhante" :body [120 255 200] :belly [230 255 240] :place "cave"   :weight 6}
   {:id "cod"     :display "Bacalhau Gelado"   :body [180 220 255] :belly [255 255 255] :place "snow"   :weight 6}
   {:id "star"    :display "Truta-Estrela"     :body [40 40 90]    :belly [255 240 120] :place "night"  :weight 4}
   {:id "rainbow" :display "Peixe Arco-Íris"   :body [255 80 160]  :belly [120 220 255] :place "golden" :weight 1}])

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
;; Varas (um item com nome terminando em _rod joga a linha)
;; ---------------------------------------------------------------

(register-item {:name "fish:lucky_rod" :display "Vara da Sorte" :stack 1
                :picture rod-shape :colors {:r [120 80 40] :s [230 230 230] :h [80 220 80]}})
(register-item {:name "fish:golden_rod" :display "Vara Dourada" :stack 1
                :picture rod-shape :colors {:r [255 200 40] :s [255 255 200] :h [255 120 40]}})

;; Uma vara da sorte: igual à vara de pescar, com uma barra de ouro na linha.
(register-recipe {:pattern ["  s" " st" "s g"]
                  :key {:s "core:stick" :t "core:string" :g "core:gold_ingot"}
                  :result "fish:lucky_rod"})

;; Uma vara dourada: feita de ouro. Só a vara dourada pesca o Peixe Arco-Íris.
(register-recipe {:pattern ["  g" " gt" "g t"]
                  :key {:g "core:gold_ingot" :t "core:string"}
                  :result "fish:golden_rod"})

;; ---------------------------------------------------------------
;; Onde você está pescando?
;; ---------------------------------------------------------------

(def water (block-id "core:water"))

;; Pedra em cima da cabeça: uma caverna.
(defn in-a-cave? (x y z)
  (let (top (ground-height x z))
    (and top (> top (+ y 3)))))

;; Quatro blocos de água embaixo da boia: água funda.
(defn deep? (x y z)
  (every? (fn (d) (= (get-block x (- y d) z) water)) [1 2 3 4]))

;; Neve ou gelo por perto.
(def cold-blocks [(block-id "core:snow") (block-id "core:snow_block")
                  (block-id "core:ice") (block-id "core:packed_ice")])
(defn cold? (b) (some? (fn (c) (= b c)) cold-blocks))
(defn snowy? (x y z)
  (some? (fn (d)
           (or (cold? (get-block (+ x (nth d 0)) y (+ z (nth d 1))))
               (cold? (get-block (+ x (nth d 0)) (+ y 1) (+ z (nth d 1))))))
         [[1 0] [-1 0] [0 1] [0 -1] [2 0] [-2 0] [0 2] [0 -2] [1 1] [-1 -1] [1 -1] [-1 1]]))

(defn night? ()
  (let (t (time-of-day)) (or (< t 0.23) (> t 0.77))))

(defn fishing-place (x y z)
  (cond (in-a-cave? x y z) "cave"
        (snowy? x y z)     "snow"
        (deep? x y z)      "deep"
        true               "pond"))

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

(defn choose-fish (place rod)
  (let (lucky (= rod "fish:lucky_rod")
        golden (= rod "fish:golden_rod")
        here (filter (fn (f)
                       (let (p (dict-get f :place))
                         (or (= p place)
                             (and (= p "night") (night?))
                             (and (= p "golden") golden))))
                     fish-list))
    ;; A vara da sorte deixa os peixes raros (peso menor que 5) três vezes mais fáceis.
    (weighted-pick
      (map (fn (f)
             (if (and lucky (< (dict-get f :weight) 5))
                 (dict-set f :weight (* 3 (dict-get f :weight)))
                 f))
           here))))

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "Você pescou uma Bota Velha. Eca!")
          "fish:old_boot")
      (let (f (choose-fish (fishing-place x y z) rod))
        (tell player (str "Você pescou: " (dict-get f :display) "!"))
        (fish-item f))))

(defn on-join (player name)
  (tell player (str "Olá, " name "! A Pesca Divertida está ligada.")))

(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player "Fabrique uma Vara da Sorte ou uma Vara Dourada. Só a Vara Dourada pesca o Peixe Arco-Íris!")
    true))
