radio-fm deployment bundle (Linux x86_64)

1) Copy this dist folder (or tar.gz) to the target machine.

2) Install runtime dependencies on the target machine:
sudo pacman -S --needed gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav gst-plugin-pipewire pipewire pipewire-pulse alsa-lib

3) Make binary executable:
chmod +x radio-fm

4) Run:
./radio-fm --help
./radio-fm scan /path/to/music --json
./radio-fm play /path/to/file.mp3

Notes:
- This binary is built on Manjaro/Arch and is intended for compatible Linux x86_64 systems.
- For Debian/Ubuntu/Fedora targets, rebuild there or use a containerized build per distro.
