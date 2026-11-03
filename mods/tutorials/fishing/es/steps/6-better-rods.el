;; Pesca Divertida, paso 6: cañas mejores.

;; ---------------------------------------------------------------
;; Dibujos
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
;; Los peces
;; ---------------------------------------------------------------

;; :weight dice cuánto pica: 10 pica diez veces más que 1.
(def fish-list
  [{:id "perch"   :display "Perca del Charco"   :body [240 170 60]  :belly [255 230 150] :place "pond"   :weight 10}
   {:id "carp"    :display "Carpa Burbuja"      :body [150 120 200] :belly [220 200 255] :place "pond"   :weight 6}
   {:id "tuna"    :display "Atún Azul Gigante"  :body [40 80 180]   :belly [200 220 240] :place "deep"   :weight 6}
   {:id "minnow"  :display "Pececillo Luminoso" :body [120 255 200] :belly [230 255 240] :place "cave"   :weight 6}
   {:id "cod"     :display "Bacalao Escarchado" :body [180 220 255] :belly [255 255 255] :place "snow"   :weight 6}
   {:id "star"    :display "Trucha Estrellada"  :body [40 40 90]    :belly [255 240 120] :place "night"  :weight 4}
   {:id "rainbow" :display "Pez Arcoíris"       :body [255 80 160]  :belly [120 220 255] :place "golden" :weight 1}])

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
                  :display (str (dict-get f :display) " a la brasa")
                  :picture fish-shape
                  :colors {:b [170 100 40] :w [220 160 90] :e [60 30 10] :t [140 80 30]}
                  :food 6}))

(register-item {:name "fish:old_boot" :display "Bota Vieja"
                :picture boot-shape :colors {:b [110 70 40] :s [60 40 20]}})

;; ---------------------------------------------------------------
;; Cañas (un objeto cuyo nombre acaba en _rod lanza el sedal)
;; ---------------------------------------------------------------

(register-item {:name "fish:lucky_rod" :display "Caña de la Suerte" :stack 1
                :picture rod-shape :colors {:r [120 80 40] :s [230 230 230] :h [80 220 80]}})
(register-item {:name "fish:golden_rod" :display "Caña Dorada" :stack 1
                :picture rod-shape :colors {:r [255 200 40] :s [255 255 200] :h [255 120 40]}})

;; Una caña de la suerte: como una caña de pescar, con un lingote de oro en el sedal.
(register-recipe {:pattern ["  s" " st" "s g"]
                  :key {:s "core:stick" :t "core:string" :g "core:gold_ingot"}
                  :result "fish:lucky_rod"})

;; Una caña dorada: hecha de oro. Solo una caña dorada pesca el Pez Arcoíris.
(register-recipe {:pattern ["  g" " gt" "g t"]
                  :key {:g "core:gold_ingot" :t "core:string"}
                  :result "fish:golden_rod"})

;; ---------------------------------------------------------------
;; ¿Dónde estás pescando?
;; ---------------------------------------------------------------

(def water (block-id "core:water"))

;; Roca encima de tu cabeza: una cueva.
(defn in-a-cave? (x y z)
  (let (top (ground-height x z))
    (and top (> top (+ y 3)))))

;; Cuatro bloques de agua bajo el flotador: agua profunda.
(defn deep? (x y z)
  (every? (fn (d) (= (get-block x (- y d) z) water)) [1 2 3 4]))

;; Nieve o hielo cerca.
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
;; ¿Qué pica?
;; ---------------------------------------------------------------

;; Uno de los peces, elegido según su peso.
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
    ;; Con una caña de la suerte, los peces raros (peso menor que 5) salen el triple de veces.
    (weighted-pick
      (map (fn (f)
             (if (and lucky (< (dict-get f :weight) 5))
                 (dict-set f :weight (* 3 (dict-get f :weight)))
                 f))
           here))))

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "Has sacado una Bota Vieja. ¡Puaj!")
          "fish:old_boot")
      (let (f (choose-fish (fishing-place x y z) rod))
        (tell player (str "¡Has pescado: " (dict-get f :display) "!"))
        (fish-item f))))

(defn on-join (player name)
  (tell player (str "¡Hola, " name "! Pesca Divertida está en marcha.")))

(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player "Fabrica una Caña de la Suerte o una Caña Dorada. ¡Solo la Caña Dorada pesca el Pez Arcoíris!")
    true))
