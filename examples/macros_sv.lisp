(defmacro increment (value)
  (quasiquote
    (+ (unquote value)
       (meta ((width 8)) 1))))

(module macro_adder
  (ports
    (input  (meta ((width 8)) a))
    (output (meta ((width 8)) y)))
  (assign y (increment a)))
