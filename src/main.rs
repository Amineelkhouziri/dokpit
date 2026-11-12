mod context;
mod core;
mod docker;
mod store;
mod ui;

use relm4::adw;

fn main() {
    let app = relm4::RelmApp::new("io.github.dockpit");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
    app.run::<ui::window::App>(());
}
