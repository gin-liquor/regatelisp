(loop (i 0 10)
  (if (= i 5)
      (break i)
      (print "{}\n" i)))

(loop
  ((i 0)
   (sum 0))
  (while (< i 5))
  (next
    ((i (+ i 1))
     (sum (+ sum i))))
  (do
    (print "i={} sum={}\n" i sum)))
