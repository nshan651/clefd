(use-modules (guix packages)
             (gnu packages admin)
             (gnu packages linux)
             (gnu packages commencement))

(specifications->manifest
 '("gcc-toolchain"
   "make"
   "git"
   "pkg-config"
   "libinput"
   "libxkbcommon"))
