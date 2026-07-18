(module vector_ops
  (ports
    (input  (meta ((width 1)) clk))
    (input  (meta ((width 16)) word))
    (input  (meta ((width 8) (signed true)) signed_byte))
    (output (meta ((width 4)) opcode))
    (output (meta ((width 1)) flag))
    (output (meta ((width 16)) swapped))
    (output (meta ((width 16) (signed true)) extended))
    (output (meta ((width 8)) captured)))

  (register captured)

  (assign opcode (slice word 15 12))
  (assign flag (bit word 11))
  (assign swapped
    (concat
      (slice word 7 0)
      (slice word 15 8)))
  (assign extended (resize signed_byte 16))

  (clocked (clock clk rising)
    (set captured (slice word 7 0))))
