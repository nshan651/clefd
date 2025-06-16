(use-modules (srfi srfi-1)
	     (ice-9 rdelim))

(define *fifo-path* "/tmp/clef-daemon.fifo")
(define *config-path* "/home/nick/git/clef/config.scm")
(define *keybindings* '())

(define (read-config-file filename)
  "Parse the user's config.scm file and return an association list."
  (call-with-input-file filename
    (lambda (port)
      (let loop ((exprs '()))
        (let ((expr (read port)))
          (if (eof-object? expr)
              (reverse exprs)
              (loop (cons expr exprs))))))))

(define (exec-action keypress)
  "Look up the key-symbol in the keybindings and execute the associated command."
  (let ((keymap (assoc keypress *keybindings*)))
    (when keymap
      (let ((cmd (cdr keymap)))

       (display "Executing command for key: ")
       (write keypress)
       (newline)

       (cond
	;; Command is a symbol.
	((symbol? cmd)
	 (system* (symbol->string cmd)))
	
	;; Command is a list, assume it includes the program + args.
	((list? cmd)
	 (let ((program (object->string (car cmd)))
	       (args (map object->string (cdr cmd))))
	   (apply system* program args))))))))

(define (process-keypress port)
  "Main loop to process key presses as they appear."
  (let ((line (read-line port)))
    ;; when the C daemon closes, read-line will return EOF.
    (unless (eof-object? line)
      (exec-action (string->symbol line))
      (process-keypress port))))

(define (main)
  (set! *keybindings*
	(read-config-file *config-path*))

  (call-with-input-file *fifo-path* process-keypress))

(main)
