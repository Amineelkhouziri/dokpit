# dockpit

Native GTK4 / libadwaita desktop app to manage Docker containers and compose projects.

## Install (Ubuntu 24.04)

```bash
git clone https://github.com/Hamza-WPK/dokpit.git
cd dokpit
./install.sh
```

`install.sh` installs the build dependencies, Docker (if missing), Rust (if missing), builds the app, and adds it to the app menu with its icon. Log out and back in afterwards so the `docker` group applies.

Run it from the app menu or with `dockpit`.

## Requirements

- Ubuntu 24.04 (x86_64)
- Docker running, user in the `docker` group

## Build manually

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev
cargo build --release
```
