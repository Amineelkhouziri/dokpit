mod context;
mod core;
mod docker;
mod store;
mod ui;

use relm4::adw;

fn main() {
    let app = relm4::RelmApp::new("io.github.dockpit");
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
    relm4::set_global_css(include_str!("ui/style.css"));
    app.run::<ui::window::App>(());
}
