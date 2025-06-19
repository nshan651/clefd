(define (adder x)
  (+ x 10))

(define (main)
  "Main method."
  (display "Hello World")
  (newline)

  (display "Calling another function...")
  (newline)

  (display (adder 100))
  )

(main)
