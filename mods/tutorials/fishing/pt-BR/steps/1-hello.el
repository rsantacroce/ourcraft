;; Pesca Divertida, passo 1: diga olá.
;;
;; Uma linha que começa com ; é um recado para pessoas. O jogo pula essa linha.

;; Quando alguém entra no jogo, diga olá.
(defn on-join (player name)
  (tell player (str "Olá, " name "! A Pesca Divertida está ligada.")))

;; Quando alguém escreve no chat.
(defn on-chat (player text)
  (when (= text "/pescar")
    (tell player "Logo, logo você vai pescar aqui!")
    true))
