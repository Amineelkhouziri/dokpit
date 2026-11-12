use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use bollard::models::{ContainerSummary, ContainerSummaryStateEnum as State};
use bollard::query_parameters::ListContainersOptions;
use relm4::adw::{self, prelude::*};
use relm4::gtk::glib;
use relm4::prelude::*;
use relm4::RelmRemoveAllExt;

use crate::context::{AppCtx, Kind};
use crate::core::{self, ComposeCmd, ProjectRecord};
use crate::store;
use crate::ui::confirm::{Confirm, confirm};

pub struct ProjectsView {
    ctx: AppCtx,
    store_path: Option<PathBuf>,
    remembered: Vec<ProjectRecord>,
    containers: Vec<ContainerSummary>,
    filter: String,
    list: gtk::ListBox,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Filter(String),
    Compose(ComposeCmd, String),
    RunCompose(ComposeCmd, ProjectRecord),
    Forget(String),
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Refresh,
    Loaded(Vec<ContainerSummary>),
    Composed(Result<String, String>),
}

fn label(cmd: ComposeCmd) -> &'static str {
    match cmd {
        ComposeCmd::Up => "up",
        ComposeCmd::Down => "down",
        ComposeCmd::Pull => "pull",
    }
}

impl ProjectsView {
    fn save(&self) {
        if let Some(path) = &self.store_path
            && let Err(e) = store::save(path, &self.remembered)
        {
            self.ctx.error(format!("Save {}: {e}", path.display()));
        }
    }

    fn render(&self, sender: &ComponentSender<Self>) {
        self.list.remove_all();
        let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
        for g in core::group_containers(&self.containers) {
            if let Some(name) = g.project {
                let running = g.containers.iter().filter(|c| c.state == Some(State::RUNNING)).count();
                counts.insert(name, (running, g.containers.len()));
            }
        }
        let query = self.filter.trim().to_lowercase();
        for p in core::merge_projects(core::live_projects(&self.containers), &self.remembered) {
            if !p.record.name.to_lowercase().contains(&query) {
                continue;
            }
            self.list.append(&row(&p, counts.get(&p.record.name).copied(), sender));
        }
    }
}

fn row(p: &core::Project, counts: Option<(usize, usize)>, sender: &ComponentSender<ProjectsView>) -> adw::ActionRow {
    let r = &p.record;
    let paths = if r.config_files.is_empty() {
        r.working_dir.display().to_string()
    } else {
        r.config_files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join("\n")
    };
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&r.name).as_str())
        .subtitle(glib::markup_escape_text(&paths).as_str())
        .subtitle_lines(0)
        .build();
    let (state, style) = match counts {
        Some((running, total)) if p.live => {
            let style = if running == total { "success" } else if running > 0 { "warning" } else { "dim-label" };
            (format!("{running}/{total} running"), style)
        }
        _ => ("stopped".to_string(), "dim-label"),
    };
    row.add_suffix(&gtk::Label::builder().label(state).css_classes([style]).valign(gtk::Align::Center).build());

    let has_files = !r.config_files.is_empty();
    let button = |icon: &str, tooltip: &str, sensitive: bool, msg: Msg| {
        let b = gtk::Button::builder()
            .icon_name(icon)
            .tooltip_text(tooltip)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .sensitive(sensitive)
            .build();
        let s = sender.clone();
        b.connect_clicked(move |_| s.input(msg.clone()));
        row.add_suffix(&b);
    };
    let name = r.name.clone();
    button("media-playback-start-symbolic", "docker compose up -d", has_files, Msg::Compose(ComposeCmd::Up, name.clone()));
    button("media-playback-stop-symbolic", "docker compose down", p.live, Msg::Compose(ComposeCmd::Down, name.clone()));
    button("folder-download-symbolic", "docker compose pull", has_files, Msg::Compose(ComposeCmd::Pull, name.clone()));
    button("edit-delete-symbolic", "Forget (remove from remembered projects)", !p.live, Msg::Forget(name));
    row
}

#[relm4::component(pub)]
impl Component for ProjectsView {
    type Init = AppCtx;
    type Input = Msg;
    type Output = ();
    type CommandOutput = Cmd;

    view! {
        gtk::Paned {
            #[wrap(Some)]
            set_start_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                gtk::SearchEntry {
                    set_placeholder_text: Some("Filter projects"),
                    set_margin_all: 6,
                    connect_search_changed[sender] => move |e| sender.input(Msg::Filter(e.text().into())),
                },
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    #[local_ref]
                    list -> gtk::ListBox {
                        set_margin_all: 12,
                        set_valign: gtk::Align::Start,
                        set_selection_mode: gtk::SelectionMode::None,
                        add_css_class: "boxed-list",
                    },
                },
            },
        }
    }

    fn init(ctx: AppCtx, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        ctx.on_change(&sender, &[Kind::CONTAINER], Cmd::Refresh);
        let path = store::default_path();
        let (store_path, remembered) = match store::load(&path) {
            Ok(records) => (Some(path), records),
            Err(e) => {
                ctx.error(format!("Load {}: {e} (remembered projects will not be saved)", path.display()));
                (None, Vec::new())
            }
        };
        let model = Self { ctx, store_path, remembered, containers: Vec::new(), filter: String::new(), list: gtk::ListBox::new() };
        model.render(&sender);
        let list = &model.list;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Msg, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            Msg::Filter(f) => {
                self.filter = f;
                self.render(&sender);
            }
            Msg::Compose(cmd, name) => {
                let Some(p) = core::merge_projects(core::live_projects(&self.containers), &self.remembered)
                    .into_iter()
                    .find(|p| p.record.name == name)
                else {
                    return;
                };
                let record = p.record;
                let mut items = vec![format!("Project: {}", record.name)];
                items.extend(record.config_files.iter().map(|f| format!("File: {}", f.display())));
                let req = Confirm {
                    heading: format!("Run docker compose {} on {}?", label(cmd), record.name),
                    items,
                    action: format!("Compose {}", label(cmd)),
                    option: None,
                };
                confirm(req, move |_| sender.input(Msg::RunCompose(cmd, record)));
            }
            Msg::RunCompose(cmd, record) => {
                let args = core::compose_args(&record, cmd);
                let what = format!("Compose {} {}", label(cmd), record.name);
                self.ctx.toast(format!("{what}: running"));
                sender.oneshot_command(async move {
                    let out = tokio::process::Command::new("docker").args(&args).stdin(Stdio::null()).output().await;
                    Cmd::Composed(match out {
                        Ok(o) if o.status.success() => Ok(format!("{what}: done")),
                        Ok(o) => {
                            let stderr = String::from_utf8_lossy(&o.stderr);
                            let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default();
                            Err(format!("{what}: {} {last}", o.status))
                        }
                        Err(e) => Err(format!("{what}: {e}")),
                    })
                });
            }
            Msg::Forget(name) => {
                self.remembered.retain(|r| r.name != name);
                self.save();
                self.render(&sender);
            }
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
                self.containers = containers;
                let records: Vec<ProjectRecord> =
                    core::merge_projects(core::live_projects(&self.containers), &self.remembered)
                        .into_iter()
                        .map(|p| p.record)
                        .collect();
                if records != self.remembered {
                    self.remembered = records;
                    self.save();
                }
                self.render(&sender);
            }
            Cmd::Composed(Ok(msg)) => self.ctx.toast(msg),
            Cmd::Composed(Err(msg)) => self.ctx.error(msg),
        }
    }
}
