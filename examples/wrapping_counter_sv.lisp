(module wrapping_counter
  (ports
    (input  (meta ((width 1)) clk))
    (input  (meta ((width 1)) reset))
    (input  (meta ((width 1)) enable))
    (output (meta ((width 8)) count)))

  (register count
    (clock clk rising)
    (reset reset 0)
    (enable enable)
    (next
      (if (= count 255)
          0
          (+ count 1)))))
