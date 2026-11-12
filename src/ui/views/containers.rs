mod details;
mod group;
mod rows;

use std::collections::HashSet;
use std::process::Command;

use bollard::models::ContainerSummary;
use bollard::query_parameters::{
    ListContainersOptions, RemoveContainerOptions, RestartContainerOptions, StartContainerOptions, StopContainerOptions,
};
use futures_util::future::try_join_all;
use relm4::RelmRemoveAllExt;
use relm4::gtk::{glib, prelude::*};
use relm4::prelude::*;

use crate::context::{AppCtx, Kind};
use crate::core;
use crate::ui::confirm::{Confirm, confirm};

pub struct ContainersView {
    ctx: AppCtx,
    containers: Vec<ContainerSummary>,
    filter: String,
    collapsed: HashSet<Option<String>>,
    selected: Option<String>,
    details: Controller<details::Details>,
    scroll: gtk::ScrolledWindow,
    groups: gtk::Box,
}

#[derive(Debug)]
pub enum Msg {
    Filter(String),
    Toggle(Option<String>),
    Select(String),
    Start(String),
    Stop(String),
    Restart(String),
    Remove(String),
    RemoveConfirmed { id: String, force: bool },
    Shell(String),
    Group(Op, String),
}

#[derive(Debug, Clone, Copy)]
pub enum Op {
    Start,
    Stop,
    Restart,
    Remove,
}

pub fn run(
    ctx: &AppCtx,
    sender: &ComponentSender<ContainersView>,
    op: Op,
    ids: Vec<String>,
    force: bool,
    what: &'static str,
    done: &'static str,
) {
    ctx.run(
        sender,
        what,
        move |d| async move {
            try_join_all(ids.iter().map(|id| {
                let d = d.clone();
                async move {
                    match op {
                        Op::Start => d.start_container(id, None::<StartContainerOptions>).await,
                        Op::Stop => d.stop_container(id, None::<StopContainerOptions>).await,
                        Op::Restart => d.restart_container(id, None::<RestartContainerOptions>).await,
                        Op::Remove => {
                            let opts = RemoveContainerOptions { force, ..Default::default() };
                            d.remove_container(id, Some(opts)).await
                        }
                    }
                }
            }))
            .await
        },
        move |_| Cmd::Done(done),
    );
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Refresh,
    Loaded(Vec<ContainerSummary>),
    Done(&'static str),
}

impl ContainersView {
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    fn set_selected(&mut self, id: Option<String>) {
        if self.selected == id {
            return;
        }
        self.details.emit(details::Msg::Show(id.clone()));
        self.selected = id;
    }

    fn render(&self, sender: &ComponentSender<Self>) {
        let adj = self.scroll.vadjustment();
        let pos = adj.value();
        self.groups.remove_all();
        let filtering = !self.filter.trim().is_empty();
        for group in core::filter_groups(core::group_containers(&self.containers), &self.filter) {
            let open = filtering || !self.collapsed.contains(&group.project);
            self.groups.append(&rows::group_header(&group, open, !filtering, sender));
            if open {
                self.groups.append(&rows::group_list(&group, self.selected(), sender));
            }
        }
        adj.set_value(pos);
    }

    fn shell(&self, id: &str) {
        let Some(argv) = core::shell_argv(self.ctx.host(), id) else {
            self.ctx.error(format!("Open shell: invalid container id {id}"));
            return;
        };
        let command = argv.iter().map(|a| glib::shell_quote(a).to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        match Command::new("tilix").args(["-e", &command]).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(e) => self.ctx.error(format!("Open shell: {e}")),
        }
    }
}

#[relm4::component(pub)]
impl Component for ContainersView {
    type Init = AppCtx;
    type Input = Msg;
    type Output = ();
    type CommandOutput = Cmd;

    view! {
        gtk::Paned {
            set_position: 640,
            set_shrink_start_child: false,
            set_shrink_end_child: false,
            #[wrap(Some)]
            set_start_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                gtk::SearchEntry {
                    set_placeholder_text: Some("Filter by name, image or project"),
                    set_margin_all: 6,
                    connect_search_changed[sender] => move |e| sender.input(Msg::Filter(e.text().into())),
                },
                #[local_ref]
                scroll -> gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                },
            },
            set_end_child: Some(model.details.widget()),
        }
    }

    fn init(ctx: AppCtx, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        ctx.on_change(&sender, &[Kind::CONTAINER], Cmd::Refresh);
        let groups = gtk::Box::new(gtk::Orientation::Vertical, 6);
        groups.set_margin_all(12);
        let scroll = gtk::ScrolledWindow::builder().child(&groups).build();
        let details = details::Details::builder().launch(ctx.clone()).detach();
        let model = Self {
            ctx,
            containers: Vec::new(),
            filter: String::new(),
            collapsed: HashSet::new(),
            selected: None,
            details,
            scroll: scroll.clone(),
            groups,
        };
        let scroll = &scroll;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Msg, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            Msg::Filter(f) => {
                self.filter = f;
                self.render(&sender);
            }
            Msg::Toggle(project) => {
                if !self.collapsed.remove(&project) {
                    self.collapsed.insert(project);
                }
                self.render(&sender);
            }
            Msg::Select(id) => self.set_selected(Some(id)),
            Msg::Start(id) => run(&self.ctx, &sender, Op::Start, vec![id], false, "Start container", "Container started"),
            Msg::Stop(id) => run(&self.ctx, &sender, Op::Stop, vec![id], false, "Stop container", "Container stopped"),
            Msg::Restart(id) => {
                run(&self.ctx, &sender, Op::Restart, vec![id], false, "Restart container", "Container restarted")
            }
            Msg::Remove(id) => {
                let Some(c) = self.containers.iter().find(|c| c.id.as_deref() == Some(id.as_str())) else { return };
                let req = Confirm {
                    heading: "Remove container?".into(),
                    items: vec![core::display_name(c)],
                    action: "Remove".into(),
                    option: core::is_up(c).then(|| "Force (container is running)".into()),
                    destructive: true,
                };
                confirm(req, move |force| sender.input(Msg::RemoveConfirmed { id, force }));
            }
            Msg::RemoveConfirmed { id, force } => {
                run(&self.ctx, &sender, Op::Remove, vec![id], force, "Remove container", "Container removed")
            }
            Msg::Shell(id) => self.shell(&id),
            Msg::Group(op, project) => group::request(self, op, &project, &sender),
        }
    }

    fn update_cmd(&mut self, cmd: Cmd, sender: ComponentSender<Self>, _root: &Self::Root) {
        match cmd {
            Cmd::Refresh => self.ctx.run(
                &sender,
                "List containers",
                |d| async move { d.list_containers(Some(ListContainersOptions { all: true, ..Default::default() })).await },
                Cmd::Loaded,
            ),
            Cmd::Loaded(containers) if containers == self.containers => {}
            Cmd::Loaded(containers) => {
                if let Some(id) = self.selected()
                    && !containers.iter().any(|c| c.id.as_deref() == Some(id))
                {
                    self.set_selected(None);
                }
                self.containers = containers;
                self.render(&sender);
            }
            Cmd::Done(msg) => self.ctx.toast(msg),
        }
    }
}
