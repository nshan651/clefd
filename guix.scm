;;; Use this guix manifest to create an isolated shell or custom development profile.
;;; `guix shell -m guix.scm' or `guix package -m guix.scm'.
(use-modules (guix packages))

(specifications->manifest
 '("make"
   "rust"
   "cargo-tarpaulin"
   "pkg-config"
   "libinput"
   "eudev"
   "libxkbcommon"))
