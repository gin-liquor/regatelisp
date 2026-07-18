(defmacro begin (&rest forms)
  (quasiquote
    (do
      (unquote-splicing forms))))

(defmacro invoke (operator &rest operands)
  (cons operator operands))

(begin)

(begin
  (invoke + 1 2)
  (invoke + 20 22))

(list
  (quote alpha)
  (quote beta))

(cons
  (quote first)
  (append
    (quote (second))
    (quote (third))))

(quasiquote
  (outer
    (quasiquote
      (inner
        (unquote-splicing still-protected)))
    (unquote-splicing
      (quote (a b)))))
