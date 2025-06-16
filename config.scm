 ;; Single assoc t
 (F5 . pavucontrol)

 ;; Single key, multiple commands in the value are wrapped in parens.
 (XF86AudioLowerVolume . (pactl set-sink-volume 0 -5%))

 (XF86AudioRaiseVolume . (pactl set-sink-volume 0 +5%))

 (XF86AudioMute . (pactl set-sink-mute 0 toggle))

 ;; Multiple key chords wrapped in parens, value is alone so leave it as-is.
 ((Super_L w) . firefox)

 ((Super_L Shift_L w) . (dmenu --help))

(F4 . (echo "Hello World!"))

(F9 . (guix shell --help))

((Control_L F3) . (echo "Hello Keychording!"))
