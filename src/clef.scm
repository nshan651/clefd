(use-modules (srfi srfi-1))

(define *keybindings* (make-hash-table))
(define *raw-kbd-data* '())

(define (read-config-file filename)
  "Parse the user's config.scm file."
  (with-input-from-file filename
    (lambda ()
      (let loop ((keybindings '()))
	(let ((expr (read)))
	      (if (eof-object? expr)
		  (reverse keybindings)
		  (loop (cons expr keybindings))))))))

;;; Alternatively, we can use `call-with-input-file' on an entire list in one pass!
;;; Just enclose the definitions with quote, quasiquote, or `list'.
;;; We can also call `eval' on this.
;; (define (read-config-file filename)
;;   (call-with-input-file filename read))

(define (exec-action keypress)
  (let ((keymap (assoc keypress *raw-kbd-data*)))
    (when keymap
      (let ((cmd (cdr keymap)))
	(cond
	 ;; Command is a symbol.
	 ((symbol? cmd)
	  (system* (symbol->string cmd)))

	 ;; Command is a list, assume it includes the program + args.
	 ((list? cmd)
	  (let ((program (object->string (car cmd)))
		(args (map object->string (cdr cmd))))
	    (apply system* program args))))))))

(define (main)
  (set! *raw-kbd-data*
	(read-config-file "/home/nick/git/clef/config.scm"))

  (write *raw-kbd-data*)
  (newline)

  ;; (define x (assoc '(Super_L Shift_L w) *raw-kbd-data*))
  ;; (define x (assoc 'XF86AudioMute *raw-kbd-data*))
  ;; (define x (assoc 'F5 *raw-kbd-data*))
  ;; (write (cdr x))

  (exec-action 'F9)

  )

(main)
