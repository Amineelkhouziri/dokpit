use std::cell::Cell;

use relm4::gtk::{self, prelude::*};

pub struct Confirm {
    pub heading: String,
    pub items: Vec<String>,
    pub action: String,
    pub option: Option<String>,
}

#[allow(dead_code)]
pub fn confirm(req: Confirm, on_accept: impl FnOnce(bool) + 'static) {
    let dialog = gtk::MessageDialog::builder()
        .modal(true)
        .message_type(gtk::MessageType::Question)
        .buttons(gtk::ButtonsType::None)
        .text(req.heading.as_str())
        .build();
    if let Some(win) = relm4::main_application().active_window() {
        dialog.set_transient_for(Some(&win));
    }
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog
        .add_button(&req.action, gtk::ResponseType::Accept)
        .add_css_class("destructive-action");
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let area = dialog.message_area().downcast::<gtk::Box>().expect("message area is a GtkBox");
    let list = gtk::Label::builder()
        .label(req.items.join("\n"))
        .xalign(0.0)
        .selectable(true)
        .build();
    area.append(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(240)
            .propagate_natural_height(true)
            .child(&list)
            .build(),
    );
    let check = req.option.map(|label| {
        let check = gtk::CheckButton::with_label(&label);
        area.append(&check);
        check
    });

    let on_accept = Cell::new(Some(on_accept));
    dialog.connect_response(move |dialog, response| {
        dialog.destroy();
        if response == gtk::ResponseType::Accept
            && let Some(f) = on_accept.take()
        {
            f(check.as_ref().is_some_and(|c| c.is_active()));
        }
    });
    dialog.present();
}
