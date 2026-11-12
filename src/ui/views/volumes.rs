use bollard::query_parameters::{ListVolumesOptions};
use relm4::adw;
use relm4::prelude::*;

use crate::context::{AppCtx, Kind};
use crate::ui::detail;

pub struct VolumesView {
    ctx: AppCtx,
    count: Option<usize>,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Refresh,
    Loaded(usize),
}

#[relm4::component(pub)]
impl Component for VolumesView {
    type Init = AppCtx;
    type Input = ();
    type Output = ();
    type CommandOutput = Cmd;

    view! {
        gtk::Paned {
            set_position: 560,
            set_shrink_start_child: false,
            set_shrink_end_child: false,
            #[wrap(Some)]
            set_start_child = &adw::StatusPage {
                set_icon_name: Some("drive-harddisk-symbolic"),
                set_title: "Volumes",
                #[watch]
                set_description: model.count.map(|n| format!("{n} volumes")).as_deref(),
            },
            #[wrap(Some)]
            set_end_child = &detail::placeholder(),
        }
    }

    fn init(ctx: AppCtx, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        ctx.on_change(&sender, &[Kind::VOLUME], Cmd::Refresh);
        let model = Self { ctx, count: None };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update_cmd(&mut self, cmd: Cmd, sender: ComponentSender<Self>, _root: &Self::Root) {
        match cmd {
            Cmd::Refresh => self.ctx.run(
                &sender,
                "List volumes",
                |d| async move { d.list_volumes(None::<ListVolumesOptions>).await },
                |v| Cmd::Loaded(v.volumes.map_or(0, |v| v.len())),
            ),
            Cmd::Loaded(n) => self.count = Some(n),
        }
    }
}
