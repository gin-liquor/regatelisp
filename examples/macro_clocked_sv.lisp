(defmacro clocked-rising (clk &rest statements)
  (quasiquote
    (clocked
      (clock (unquote clk) rising)
      (unquote-splicing statements))))

(module macro_counter
  (ports
    (input  (meta ((width 1)) clk))
    (output (meta ((width 8)) count))
    (output (meta ((width 1)) pulse)))

  (register count)
  (register pulse)

  (clocked-rising clk
    (set count
      (+ count (meta ((width 8)) 1)))
    (set pulse
      (bit count 0))))
