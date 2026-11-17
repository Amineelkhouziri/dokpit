#!/usr/bin/env bash
set -e
cd "$(dirname "$0")"

sudo apt-get update
sudo apt-get install -y build-essential pkg-config curl libgtk-4-dev libadwaita-1-dev

if ! command -v docker >/dev/null 2>&1; then
  sudo apt-get install -y docker.io
fi
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"

if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
. "$HOME/.cargo/env"
rustup update stable

cargo build --release

install -Dm755 target/release/dockpit "$HOME/.local/bin/dockpit"
install -Dm644 data/io.github.dockpit.svg "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.dockpit.svg"
mkdir -p "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/io.github.dockpit.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=dockpit
Comment=Manage Docker containers
Exec=$HOME/.local/bin/dockpit
Icon=io.github.dockpit
Categories=System;
StartupWMClass=io.github.dockpit
DESK
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

echo "Done. Log out and back in (docker group), then launch dockpit from the app menu."
