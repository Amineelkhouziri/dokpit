use std::cell::Cell;

use relm4::gtk::{self, prelude::*};

const DRIVERS: [&str; 3] = ["bridge", "macvlan", "ipvlan"];

pub fn ask(on_ok: impl FnOnce(String, String) + 'static) {
    let dialog = gtk::MessageDialog::builder()
        .modal(true)
        .buttons(gtk::ButtonsType::None)
        .text("Create network")
        .build();
    if let Some(win) = relm4::main_application().active_window() {
        dialog.set_transient_for(Some(&win));
    }
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog
        .add_button("Create", gtk::ResponseType::Accept)
        .add_css_class("suggested-action");
    dialog.set_default_response(gtk::ResponseType::Accept);

    let area = dialog.message_area().downcast::<gtk::Box>().expect("message area is a GtkBox");
    let name = gtk::Entry::builder().placeholder_text("Name").activates_default(true).build();
    let driver = gtk::DropDown::from_strings(&DRIVERS);
    area.append(&name);
    area.append(&driver);

    let on_ok = Cell::new(Some(on_ok));
    dialog.connect_response(move |dialog, response| {
        let net = name.text().trim().to_string();
        dialog.destroy();
        if response == gtk::ResponseType::Accept
            && !net.is_empty()
            && let Some(f) = on_ok.take()
        {
            f(net, DRIVERS[driver.selected() as usize].to_string());
        }
    });
    dialog.present();
}
