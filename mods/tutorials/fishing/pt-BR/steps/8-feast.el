;; Pesca Divertida: passo 8, o mod completo.
;;
;; Peixes de vários tipos, lugares para achar cada um, varas melhores, um mercado
;; de peixe, comida, um banquete e um diário de pesca.

;; ---------------------------------------------------------------
;; Desenhos
;; ---------------------------------------------------------------

;; Todo peixe tem o mesmo desenho. Cada letra é uma cor:
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
;; Os peixes
;; ---------------------------------------------------------------

;; :weight diz quantas vezes ele morde a isca: 10 morde dez vezes mais que 1.
;; :price é quantas moedas ele vale no mercado (peixe cozido vale o dobro).
(def fish-list
  [{:id "perch"   :display "Perca-da-Poça"     :body [240 170 60]  :belly [255 230 150] :place "pond"   :weight 10 :price 2}
   {:id "carp"    :display "Carpa-Bolha"       :body [150 120 200] :belly [220 200 255] :place "pond"   :weight 6  :price 3}
   {:id "tuna"    :display "Atum Azulão"       :body [40 80 180]   :belly [200 220 240] :place "deep"   :weight 6  :price 5}
   {:id "minnow"  :display "Lambari Brilhante" :body [120 255 200] :belly [230 255 240] :place "cave"   :weight 6  :price 6}
   {:id "cod"     :display "Bacalhau Gelado"   :body [180 220 255] :belly [255 255 255] :place "snow"   :weight 6  :price 6}
   {:id "star"    :display "Truta-Estrela"     :body [40 40 90]    :belly [255 240 120] :place "night"  :weight 4  :price 8}
   {:id "rainbow" :display "Peixe Arco-Íris"   :body [255 80 160]  :belly [120 220 255] :place "golden" :weight 1  :price 50}])

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

(register-item {:name "fish:coin" :display "Moeda de Peixe"
                :picture coin-shape :colors {:y [255 210 60] :o [255 250 200]}})

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
;; O banquete
;; ---------------------------------------------------------------

(register-item {:name "fish:sandwich" :display "Sanduíche de Peixe" :food 12 :heals 4
                :picture ["........"
                          ".bbbbbb."
                          "gggggggg"
                          ".ffffff."
                          "bbbbbbbb"
                          ".bbbbbb."]
                :colors {:b [220 170 90] :g [90 200 70] :f [200 120 60]}})
(register-recipe {:ingredients ["core:bread" "fish:cooked_perch"] :result "fish:sandwich"})

(register-item {:name "fish:feast" :display "Banquete do Pescador" :food 20 :heals 20
                :picture ["..wwww.."
                          ".wyyyyw."
                          "wyoyyoyw"
                          "wyyyyyyw"
                          ".wwwwww."]
                :colors {:w [240 240 240] :y [250 200 90] :o [230 110 50]}})
(register-recipe {:ingredients ["fish:cooked_tuna" "fish:cooked_minnow" "fish:cooked_cod"
                                "fish:cooked_star" "core:bread"]
                  :result "fish:feast"})

;; O bloco do balcão do mercado.
(register-block {:name "fish:market" :display "Mercado de Peixe"
                 :picture fish-shape
                 :colors {:b [255 255 255] :w [200 230 255] :e [20 20 20] :t [255 255 255]}
                 :color [40 120 200]})

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

;; ---------------------------------------------------------------
;; Seu diário de pesca (guardado com o mundo)
;; ---------------------------------------------------------------

(def journals (or (load-data "journals") {}))

;; O nome de quem está jogando, que é sempre o mesmo.
(defn name-of (player)
  (dict-get (head (filter (fn (p) (= (dict-get p :id) player)) (players))) :name))

(defn journal (player)
  (or (dict-get journals (name-of player)) []))

(defn remember-catch (player f)
  (let (seen (journal player) id (dict-get f :id))
    (unless (some? (fn (s) (= s id)) seen)
      (set! journals (dict-set journals (name-of player) (cons id seen)))
      ;; Guarda para a próxima vez que o mundo abrir.
      (save-data "journals" journals)
      (show-title (str "Peixe novo: " (dict-get f :display) "!") 2 player)
      (confetti-on player))))

(defn confetti-on (player)
  (let (p (player-pos player))
    (confetti (nth p 0) (+ (nth p 1) 2) (nth p 2))))

(defn update-hud (player)
  (set-hud player
           [(str "Moedas de Peixe: " (count-item player "fish:coin"))
            (str "Peixes achados: " (length (journal player)) " de " (length fish-list))]))

;; ---------------------------------------------------------------
;; Mordeu a isca!
;; ---------------------------------------------------------------

(defn on-fish-bite (player rod x y z)
  (if (chance 8)
      (do (tell player "Você pescou uma Bota Velha. Eca!")
          "fish:old_boot")
      (let (f (choose-fish (fishing-place x y z) rod))
        (tell player (str "Você pescou: " (dict-get f :display) "!"))
        (remember-catch player f)
        (update-hud player)
        (fish-item f))))

;; ---------------------------------------------------------------
;; O mercado de peixe
;; ---------------------------------------------------------------

;; /mercado monta uma barraquinha do seu lado.
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
    (tell player "O Mercado de Peixe está aberto! Fique do lado dele e digite /vender")))

;; Tem um balcão de mercado perto do jogador?
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

;; Vende todos os peixes que você carrega.
(defn sell-everything (player)
  (let (coins 0)
    (for-each f fish-list
      (let (raw (take-item player (fish-item f) 999)
            cooked (take-item player (cooked-item f) 999))
        (set! coins (+ coins (* raw (dict-get f :price))
                             (* cooked 2 (dict-get f :price))))))
    (set! coins (+ coins (take-item player "fish:old_boot" 999)))
    (if (= coins 0)
        (tell player "Você não tem peixe para vender. Vá pescar!")
        (do (give-item player "fish:coin" coins)
            (sound "pop")
            (tell player (str "Você vendeu seus peixes por " coins " Moedas de Peixe!"))))
    (update-hud player)))

;; Coisas que o mercado vende: [o-quê preço nome].
(def shop
  [["fish:golden_rod" 30 "Vara Dourada (pesca o Peixe Arco-Íris!)"]
   ["core:fishing_rod" 3 "Vara de pescar"]
   ["core:bread" 2 "Pão"]
   ["core:lantern" 4 "Lanterna"]])

(defn show-shop (player)
  (tell player "O mercado vende (digite /comprar e o número):")
  (for-each i (range (length shop))
    (let (thing (nth shop i))
      (tell player (str "  " (+ i 1) ". " (nth thing 2) " - " (nth thing 1) " moedas")))))

(defn buy (player number)
  (let (thing (if (> number 0) (nth shop (- number 1)) nil))
    (cond (nil? thing)
          (tell player "Não tem nada com esse número. Digite /loja")
          (< (count-item player "fish:coin") (nth thing 1))
          (tell player (str "Você precisa de " (nth thing 1) " Moedas de Peixe para isso."))
          true
          (do (take-item player "fish:coin" (nth thing 1))
              (give-item player (nth thing 0))
              (sound "pop")
              (tell player (str "Você comprou: " (nth thing 2)))
              (update-hud player)))))

;; ---------------------------------------------------------------
;; Comandos do chat
;; ---------------------------------------------------------------

;; "2" vira 2. O que não for número vira 0.
(defn as-number (word)
  (if (and word (str-matches word "^[0-9]+$")) (->number word) 0))

(defn on-join (player name)
  (tell player (str "Que bom te ver, " name "! A Pesca Divertida está ligada. Digite /pescar para ver a ajuda."))
  (update-hud player))

(defn on-chat (player text)
  (let (words (str-split (str-trim text) " ")
        command (head words))
    (cond
      (= command "/pescar")
      (do (tell player "Pesque com uma vara de pescar: aponte para a água e clique com o botão direito.")
          (tell player "/mercado monta um mercado, /vender vende seus peixes,")
          (tell player "/loja mostra o que você pode comprar, /diario mostra seus peixes.")
          true)
      (= command "/mercado") (do (build-market player) true)
      (= command "/vender")
      (do (if (near-market? player)
              (sell-everything player)
              (tell player "Fique do lado de um Mercado de Peixe para vender. (/mercado monta um)"))
          true)
      (= command "/loja") (do (show-shop player) true)
      (= command "/comprar")
      (do (if (near-market? player)
              (buy player (as-number (nth words 1)))
              (tell player "Fique do lado de um Mercado de Peixe para comprar."))
          true)
      (= command "/diario")
      (do (tell player (str "Você já achou " (length (journal player)) " de "
                            (length fish-list) " peixes:"))
          (for-each f fish-list
            (when (some? (fn (s) (= s (dict-get f :id))) (journal player))
              (tell player (str "  " (dict-get f :display)))))
          true)
      true nil)))
