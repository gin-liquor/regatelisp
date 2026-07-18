; Stage 9: macros receive source arguments as Datum values and return code data.
(defmacro when (condition body)
  (quasiquote
    (if (unquote condition)
        (unquote body)
        0)))

(defmacro twice (value)
  (let ((temporary (gensym (quote temporary))))
    (quasiquote
      (let (((unquote temporary) (unquote value)))
        (+ (unquote temporary) (unquote temporary))))))

(when true (twice 21))
