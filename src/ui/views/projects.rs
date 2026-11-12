use relm4::adw;
use relm4::prelude::*;

use crate::context::AppCtx;
use crate::ui::detail;

pub struct ProjectsView {
    _ctx: AppCtx,
}

#[relm4::component(pub)]
impl SimpleComponent for ProjectsView {
    type Init = AppCtx;
    type Input = ();
    type Output = ();

    view! {
        gtk::Paned {
            set_position: 560,
            set_shrink_start_child: false,
            set_shrink_end_child: false,
            #[wrap(Some)]
            set_start_child = &adw::StatusPage {
                set_icon_name: Some("folder-symbolic"),
                set_title: "Projects",
            },
            #[wrap(Some)]
            set_end_child = &detail::placeholder(),
        }
    }

    fn init(ctx: AppCtx, root: Self::Root, _sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let model = Self { _ctx: ctx };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }
}
