use relm4::adw;

pub fn placeholder() -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name("view-paged-symbolic")
        .title("No selection")
        .description("Select an item to see its details")
        .build()
}
