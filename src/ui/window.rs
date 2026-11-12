use relm4::adw::{self, prelude::*};
use relm4::prelude::*;

use crate::context::{AppCtx, Notice};
use crate::docker::{self, Status};
use crate::ui::views::{
    containers::ContainersView, images::ImagesView, networks::NetworksView, projects::ProjectsView,
    volumes::VolumesView,
};

#[derive(PartialEq)]
enum Conn {
    Connecting,
    Up,
    Down(String),
}

struct Views {
    _containers: Controller<ContainersView>,
    _images: Controller<ImagesView>,
    _volumes: Controller<VolumesView>,
    _networks: Controller<NetworksView>,
    _projects: Controller<ProjectsView>,
}

pub struct App {
    ctx: AppCtx,
    conn: Conn,
    error: Option<String>,
    _views: Views,
}

#[derive(Debug)]
pub enum AppMsg {
    Notice(Notice),
    DismissError,
}

impl App {
    fn page(&self) -> &'static str {
        match self.conn {
            Conn::Connecting => "connecting",
            Conn::Up => "main",
            Conn::Down(_) => "down",
        }
    }

    fn down_reason(&self) -> String {
        match &self.conn {
            Conn::Down(e) => format!("{e}\nRetrying every 3 seconds…"),
            _ => String::new(),
        }
    }
}

#[relm4::component(pub)]
impl Component for App {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = Status;

    view! {
        adw::ApplicationWindow {
            set_title: Some("dockpit"),
            set_default_size: (1200, 760),

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                adw::HeaderBar {},

                gtk::InfoBar {
                    set_message_type: gtk::MessageType::Error,
                    set_show_close_button: true,
                    #[watch]
                    set_revealed: model.error.is_some(),
                    add_child = &gtk::Label {
                        set_xalign: 0.0,
                        set_wrap: true,
                        set_selectable: true,
                        #[watch]
                        set_label: model.error.as_deref().unwrap_or_default(),
                    },
                    connect_response[sender] => move |_, _| sender.input(AppMsg::DismissError),
                },

                #[name = "toasts"]
                adw::ToastOverlay {
                    set_vexpand: true,

                    #[wrap(Some)]
                    set_child = &gtk::Stack {
                        set_transition_type: gtk::StackTransitionType::Crossfade,
                        add_named[Some("connecting")] = &adw::StatusPage {
                            set_icon_name: Some("network-transmit-receive-symbolic"),
                            set_title: "Connecting to Docker…",
                        },
                        add_named[Some("down")] = &adw::StatusPage {
                            set_icon_name: Some("network-offline-symbolic"),
                            set_title: "Docker not reachable",
                            #[watch]
                            set_description: Some(&model.down_reason()),
                        },
                        add_named[Some("main")] = &gtk::Box {
                            gtk::StackSidebar {
                                set_width_request: 180,
                                set_stack: &views_stack,
                            },
                            gtk::Separator {},
                            #[local_ref]
                            views_stack -> gtk::Stack {
                                set_hexpand: true,
                            },
                        },

                        #[watch]
                        set_visible_child_name: model.page(),
                    },
                },
            },
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let (notices, notices_rx) = relm4::channel();
        let ctx = AppCtx::new(notices);
        relm4::spawn_local(notices_rx.forward(sender.input_sender().clone(), AppMsg::Notice));

        let views_stack = gtk::Stack::new();
        let add = |widget: &gtk::Paned, name: &str, title: &str| {
            views_stack.add_titled(widget, Some(name), title);
        };
        let views = Views {
            _containers: ContainersView::builder().launch(ctx.clone()).detach(),
            _images: ImagesView::builder().launch(ctx.clone()).detach(),
            _volumes: VolumesView::builder().launch(ctx.clone()).detach(),
            _networks: NetworksView::builder().launch(ctx.clone()).detach(),
            _projects: ProjectsView::builder().launch(ctx.clone()).detach(),
        };
        add(views._containers.widget(), "containers", "Containers");
        add(views._images.widget(), "images", "Images");
        add(views._volumes.widget(), "volumes", "Volumes");
        add(views._networks.widget(), "networks", "Networks");
        add(views._projects.widget(), "projects", "Projects");

        let watch_ctx = ctx.clone();
        sender.command(move |out, shutdown| shutdown.register(docker::watch(watch_ctx, out)).drop_on_shutdown());

        let model = App { ctx, conn: Conn::Connecting, error: None, _views: views };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        msg: AppMsg,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            AppMsg::Notice(Notice::Error(e)) => self.error = Some(e),
            AppMsg::Notice(Notice::Toast(t)) => widgets.toasts.add_toast(adw::Toast::new(&t)),
            AppMsg::DismissError => self.error = None,
        }
        self.update_view(widgets, sender);
    }

    fn update_cmd(&mut self, status: Status, _sender: ComponentSender<Self>, _root: &Self::Root) {
        let next = match status {
            Status::Up => Conn::Up,
            Status::Down(e) => Conn::Down(e),
        };
        if matches!(self.conn, Conn::Down(_)) && next == Conn::Up {
            self.ctx.toast("Connected to Docker");
        }
        self.conn = next;
    }
}
