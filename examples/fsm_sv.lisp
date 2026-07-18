(module controller
  (ports
    (input  (meta ((width 1)) clk))
    (input  (meta ((width 1)) reset))
    (input  (meta ((width 1)) start))
    (input  (meta ((width 1)) finished))
    (output (meta ((width 2)) state))
    (output (meta ((width 1)) busy))
    (output (meta ((width 2)) status)))

  (register state)
  (register busy)

  (assign status
    (case state
      (STATE_IDLE 0)
      (STATE_RUN 1)
      (STATE_DONE 2)
      (else 3)))

  (clocked (clock clk rising)
    (if reset
        (do
          (set state STATE_IDLE)
          (set busy 0))
        (case-do state
          (STATE_IDLE
            (if start
                (do
                  (set state STATE_RUN)
                  (set busy 1))))
          (STATE_RUN
            (if finished
                (do
                  (set state STATE_DONE)
                  (set busy 0))))
          (STATE_DONE
            (set state STATE_IDLE))
          (else
            (do
              (set state STATE_IDLE)
              (set busy 0))))))

  (enum State 2
    (STATE_IDLE 0)
    (STATE_RUN 1)
    (STATE_DONE 2)))
