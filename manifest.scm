(use-modules (guix packages)
             (gnu packages admin)    ; For pkg-config
             (gnu packages linux)    ; For libinput, libudev
             (gnu packages commencement)    ; gcc-toolchain
             )

(specifications->manifest
 '("gcc-toolchain"
   "make"
   "git"
   "pkg-config"
   "libinput"
   "libxkbcommon"))
