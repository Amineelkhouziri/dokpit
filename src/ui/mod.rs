pub mod confirm;
pub mod detail;
pub mod views;
pub mod window;

use std::path::Path;
use std::process::Command;

use relm4::gtk::{self, prelude::*};

use crate::context::AppCtx;

pub fn clear(list: &gtk::ListBox) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Opener {
    Files,
    Code,
}

impl Opener {
    fn tooltip(self) -> &'static str {
        match self {
            Opener::Files => "Open folder",
            Opener::Code => "Open in VS Code",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Opener::Files => "folder-open-symbolic",
            Opener::Code => "text-editor-symbolic",
        }
    }

    fn program(self) -> &'static str {
        match self {
            Opener::Files => "xdg-open",
            Opener::Code => "code",
        }
    }

    pub fn open(self, ctx: &AppCtx, dir: &Path) {
        match Command::new(self.program()).arg(dir).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(e) => ctx.error(format!("{}: {e}", self.tooltip())),
        }
    }

    pub fn button(self, dir: &Path, on_click: impl Fn(Opener) + 'static) -> gtk::Button {
        let found = dir.is_dir();
        let b = gtk::Button::builder()
            .icon_name(self.icon())
            .tooltip_text(if found { self.tooltip() } else { "Folder not found locally" })
            .valign(gtk::Align::Center)
            .css_classes(["flat", "dp-iconbtn"])
            .sensitive(found)
            .build();
        b.connect_clicked(move |_| on_click(self));
        b
    }
}
