(let ((value 42))
  (quasiquote
    (assign result (unquote value))))

(gensym (quote temporary))
