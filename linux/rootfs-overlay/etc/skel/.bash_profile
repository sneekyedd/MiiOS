# On the first screen, start the MiiOS desktop. (Other screens, reached with
# Ctrl+Alt+F2..F6, give a normal Linux text login for emergencies.)
if [ "$(tty)" = /dev/tty1 ] && [ -z "$WAYLAND_DISPLAY" ]; then
    exec miios-session
fi
