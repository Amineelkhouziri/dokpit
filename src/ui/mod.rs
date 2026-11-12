pub mod confirm;
pub mod detail;
pub mod views;
pub mod window;

use relm4::gtk;

pub fn clear(list: &gtk::ListBox) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
}
