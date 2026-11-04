;; Pesca Divertida: paso 8, el mod completo.
;;
;; Peces de muchos tipos, sitios donde encontrarlos, cañas mejores, un mercado de peces,
;; cocina, un banquete y un diario de pesca.

;; ---------------------------------------------------------------
;; Dibujos
;; ---------------------------------------------------------------

;; Todos los peces tienen la misma forma. Cada letra es un color:
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

(def boot-shape
  ["..bbb..."
   "..bbb..."
   "..bbb..."
   "..bbbbb."
   "..bbbbbb"
   "..ssssss"])

(def coin-shape
  ["..yyyy.."
   ".yyoyyy."
   "yyoyyyyy"
   "yyoyyyyy"
   "yyyyyyoy"
   ".yyyyoy."
   "..yyyy.."])

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
;; :price son las monedas que te dan en el mercado (el pescado cocinado vale el doble).
(def fish-list
  [{:id "perch"   :display "Perca del Charco"   :body [240 170 60]  :belly [255 230 150] :place "pond"   :weight 10 :price 2}
   {:id "carp"    :display "Carpa Burbuja"      :body [150 120 200] :belly [220 200 255] :place "pond"   :weight 6  :price 3}
   {:id "tuna"    :display "Atún Azul Gigante"  :body [40 80 180]   :belly [200 220 240] :place "deep"   :weight 6  :price 5}
   {:id "minnow"  :display "Pececillo Luminoso" :body [120 255 200] :belly [230 255 240] :place "cave"   :weight 6  :price 6}
   {:id "cod"     :display "Bacalao Escarchado" :body [180 220 255] :belly [255 255 255] :place "snow"   :weight 6  :price 6}
   {:id "star"    :display "Trucha Estrellada"  :body [40 40 90]    :belly [255 240 120] :place "night"  :weight 4  :price 8}
   {:id "rainbow" :display "Pez Arcoíris"       :body [255 80 160]  :belly [120 220 255] :place "golden" :weight 1  :price 50}])

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

(register-item {:name "fish:coin" :display "Moneda Pez"
                :picture coin-shape :colors {:y [255 210 60] :o [255 250 200]}})

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
;; El banquete
;; ---------------------------------------------------------------

(register-item {:name "fish:sandwich" :display "Bocadillo de Pescado" :food 12 :heals 4
                :picture ["........"
                          ".bbbbbb."
                          "gggggggg"
                          ".ffffff."
                          "bbbbbbbb"
                          ".bbbbbb."]
                :colors {:b [220 170 90] :g [90 200 70] :f [200 120 60]}})
(register-recipe {:ingredients ["core:bread" "fish:cooked_perch"] :result "fish:sandwich"})

(register-item {:name "fish:feast" :display "Banquete del Pescador" :food 20 :heals 20
                :picture ["..wwww.."
                          ".wyyyyw."
                          "wyoyyoyw"
                          "wyyyyyyw"
                          ".wwwwww."]
                :colors {:w [240 240 240] :y [250 200 90] :o [230 110 50]}})
(register-recipe {:ingredients ["fish:cooked_tuna" "fish:cooked_minnow" "fish:cooked_cod"
                                "fish:cooked_star" "core:bread"]
                  :result "fish:feast"})

;; El bloque del mostrador del mercado.
(register-block {:name "fish:market" :display "Mercado de Peces"
                 :picture fish-shape
                 :colors {:b [255 255 255] :w [200 230 255] :e [20 20 20] :t [255 255 255]}
                 :color [40 120 200]})

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

;; ---------------------------------------------------------------
;; Tu diario de pesca (se guarda con el mundo)
;; ---------------------------------------------------------------

(def journals (or (load-data "journals") {}))

;; El nombre de quien juega, que siempre es el mismo.
(defn name-of (player)
  (dict-get (head (filter (fn (p) (= (dict-get p :id) player)) (players))) :name))

(defn journal (player)
  (or (dict-get journals (name-of player)) []))

(defn remember-catch (player f)
  (let (seen (journal player) id (dict-get f :id))
    (unless (some? (fn (s) (= s id)) seen)
      (set! journals (dict-set journals (name-of player) (cons id seen)))
      ;; Lo guarda para la próxima vez que se abra el mundo.
      (save-data "journals" journals)
      (show-title (str "¡Pez nuevo: " (dict-get f :display) "!") 2 player)
      (confetti-on player))))

(defn confetti-on (player)
  (let (p (player-pos player))
    (confetti (nth p 0) (+ (nth p 1) 2) (nth p 2))))

(defn update-hud (player)
  (set-hud player
           [(str "Monedas Pez: " (count-item player "fish:coin"))
            (str "Peces encontrados: " (length (journal player)) " de " (length fish-list))]))

;; ---------------------------------------------------------------
;; ¡Pica!
;; ---------------------------------------------------------------

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "Has sacado una Bota Vieja. ¡Puaj!")
          "fish:old_boot")
      (let (f (choose-fish (fishing-place x y z) rod))
        (tell player (str "¡Has pescado: " (dict-get f :display) "!"))
        (remember-catch player f)
        (update-hud player)
        (fish-item f))))

;; ---------------------------------------------------------------
;; El mercado de peces
;; ---------------------------------------------------------------

;; /mercado construye un puestecito a tu lado.
(defn build-market (player)
  (let (p (player-pos player)
        x (+ (nth p 0) 2) y (nth p 1) z (nth p 2))
    (fill x (- y 1) z (+ x 2) (- y 1) (+ z 2) "core:planks")
    (fill x y z (+ x 2) (+ y 3) (+ z 2) "core:air")
    (set-block (+ x 1) y (+ z 1) "fish:market")
    (for-each c [[0 0] [2 0] [0 2] [2 2]]
      (fill (+ x (nth c 0)) y (+ z (nth c 1)) (+ x (nth c 0)) (+ y 2) (+ z (nth c 1)) "core:fence"))
    (fill x (+ y 3) z (+ x 2) (+ y 3) (+ z 2) "core:wool")
    (set-block (+ x 1) (+ y 2) (+ z 1) "core:lantern")
    (confetti (+ x 1) (+ y 1) (+ z 1))
    (tell player "¡El Mercado de Peces está abierto! Ponte a su lado y escribe /vender")))

;; ¿Hay un mostrador del mercado cerca del jugador?
(def market (block-id "fish:market"))
(defn near-market? (player)
  (let (p (player-pos player))
    (some? (fn (dx)
             (some? (fn (dz)
                      (some? (fn (dy)
                               (= (get-block (+ (nth p 0) dx) (+ (nth p 1) dy) (+ (nth p 2) dz)) market))
                             [-1 0 1]))
                    [-3 -2 -1 0 1 2 3]))
           [-3 -2 -1 0 1 2 3])))

;; Vende todos los peces que llevas.
(defn sell-everything (player)
  (let (coins 0)
    (for-each f fish-list
      (let (raw (take-item player (fish-item f) 999)
            cooked (take-item player (cooked-item f) 999))
        (set! coins (+ coins (* raw (dict-get f :price))
                             (* cooked 2 (dict-get f :price))))))
    (set! coins (+ coins (take-item player "fish:old_boot" 999)))
    (if (= coins 0)
        (tell player "No tienes peces para vender. ¡Ve a pescar!")
        (do (give-item player "fish:coin" coins)
            (sound "pop")
            (tell player (str "¡Has vendido tus peces por " coins " Monedas Pez!"))))
    (update-hud player)))

;; Lo que vende el mercado: [qué precio nombre].
(def shop
  [["fish:golden_rod" 30 "Caña Dorada (¡pesca el Pez Arcoíris!)"]
   ["core:fishing_rod" 3 "Caña de pescar"]
   ["core:bread" 2 "Pan"]
   ["core:lantern" 4 "Farol"]])

(defn show-shop (player)
  (tell player "El mercado vende (escribe /comprar y el número):")
  (for-each i (range (length shop))
    (let (thing (nth shop i))
      (tell player (str "  " (+ i 1) ". " (nth thing 2) " - " (nth thing 1) " monedas")))))

(defn buy (player number)
  (let (thing (if (> number 0) (nth shop (- number 1)) nil))
    (cond (nil? thing)
          (tell player "No hay nada con ese número. Escribe /tienda")
          (< (count-item player "fish:coin") (nth thing 1))
          (tell player (str "Necesitas " (nth thing 1) " Monedas Pez para eso."))
          true
          (do (take-item player "fish:coin" (nth thing 1))
              (give-item player (nth thing 0))
              (sound "pop")
              (tell player (str "Has comprado: " (nth thing 2)))
              (update-hud player)))))

;; ---------------------------------------------------------------
;; Comandos del chat
;; ---------------------------------------------------------------

;; "2" se convierte en 2. Lo que no sea un número se convierte en 0.
(defn as-number (word)
  (if (and word (str-matches word "^[0-9]+$")) (->number word) 0))

(defn on-join (player name)
  (tell player (str "¡Qué bien verte, " name "! Pesca Divertida está en marcha. Escribe /pescar para ver la ayuda."))
  (update-hud player))

(defn on-chat (player text)
  (let (words (str-split (str-trim text) " ")
        command (head words))
    (cond
      (= command "/pescar")
      (do (tell player "Pesca con una caña de pescar: apunta al agua y haz clic derecho.")
          (tell player "/mercado construye un mercado, /vender vende tus peces,")
          (tell player "/tienda te enseña lo que puedes comprar, /diario te enseña tus peces.")
          true)
      (= command "/mercado") (do (build-market player) true)
      (= command "/vender")
      (do (if (near-market? player)
              (sell-everything player)
              (tell player "Ponte al lado de un Mercado de Peces para vender. (/mercado construye uno)"))
          true)
      (= command "/tienda") (do (show-shop player) true)
      (= command "/comprar")
      (do (if (near-market? player)
              (buy player (as-number (nth words 1)))
              (tell player "Ponte al lado de un Mercado de Peces para comprar."))
          true)
      (= command "/diario")
      (do (tell player (str "Has encontrado " (length (journal player)) " de "
                            (length fish-list) " peces:"))
          (for-each f fish-list
            (when (some? (fn (s) (= s (dict-get f :id))) (journal player))
              (tell player (str "  " (dict-get f :display)))))
          true)
      true nil)))
