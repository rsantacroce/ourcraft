;; Pesca Divertida, paso 4: muchos peces.

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

;; ---------------------------------------------------------------
;; Los peces
;; ---------------------------------------------------------------

;; :weight dice cuánto pica: 10 pica diez veces más que 1.
(def fish-list
  [{:id "perch"   :display "Perca del Charco"   :body [240 170 60]  :belly [255 230 150] :weight 10}
   {:id "carp"    :display "Carpa Burbuja"      :body [150 120 200] :belly [220 200 255] :weight 6}
   {:id "tuna"    :display "Atún Azul Gigante"  :body [40 80 180]   :belly [200 220 240] :weight 3}
   {:id "rainbow" :display "Pez Arcoíris"       :body [255 80 160]  :belly [120 220 255] :weight 1}])

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

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "Has sacado una Bota Vieja. ¡Puaj!")
          "fish:old_boot")
      (let (f (weighted-pick fish-list))
        (tell player (str "¡Has pescado: " (dict-get f :display) "!"))
        (fish-item f))))

(defn on-join (player name)
  (tell player (str "¡Hola, " name "! Pesca Divertida está en marcha.")))

(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player (str "Hay " (length fish-list) " tipos de peces. ¿Puedes pescarlos todos?"))
    true))
