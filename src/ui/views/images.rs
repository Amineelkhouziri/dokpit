mod model;

use std::collections::HashMap;

use bollard::models::{ImagePruneResponse, ImageSummary};
use bollard::query_parameters::{
    CreateImageOptions, ListContainersOptions, ListImagesOptions, PruneImagesOptionsBuilder, RemoveImageOptions,
};
use futures_util::StreamExt;
use relm4::gtk::prelude::*;
use relm4::prelude::*;
use relm4::RelmRemoveAllExt;

use crate::context::{AppCtx, Kind};
use crate::ui::confirm::{Confirm, confirm};
use crate::ui::detail;
use model::{ImageRow, PullProgress};

pub struct ImagesView {
    ctx: AppCtx,
    rows: Vec<ImageRow>,
    shown: Vec<ImageRow>,
    filter: String,
    selected: Option<String>,
    pulling: bool,
    progress: String,
    list: gtk::ListBox,
    pull_entry: gtk::Entry,
}

#[derive(Debug)]
pub enum Msg {
    Filter(String),
    Select(Option<usize>),
    Remove,
    DoRemove { target: String, force: bool },
    Prune,
    DoPrune,
    Pull,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Refresh,
    Loaded(Vec<ImageSummary>),
    PruneList(Vec<String>),
    Pruned(ImagePruneResponse),
    Removed,
    Progress(String),
    PullDone(bool),
}

#[relm4::component(pub)]
impl Component for ImagesView {
    type Init = AppCtx;
    type Input = Msg;
    type Output = ();
    type CommandOutput = Cmd;

    view! {
        gtk::Paned {
            set_position: 560,
            set_shrink_start_child: false,
            set_shrink_end_child: false,
            #[wrap(Some)]
            set_start_child = &gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 6,
                set_margin_all: 6,

                gtk::Box {
                    set_spacing: 6,
                    gtk::SearchEntry {
                        set_hexpand: true,
                        set_placeholder_text: Some("Filter images"),
                        connect_search_changed[sender] => move |e| sender.input(Msg::Filter(e.text().into())),
                    },
                    gtk::Button {
                        set_label: "Remove",
                        #[watch]
                        set_sensitive: model.selected.is_some(),
                        connect_clicked => Msg::Remove,
                    },
                    gtk::Button {
                        set_label: "Prune",
                        set_tooltip_text: Some("Remove images not used by any container"),
                        connect_clicked => Msg::Prune,
                    },
                },
                gtk::ScrolledWindow {
                    set_vexpand: true,
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    #[local_ref]
                    list -> gtk::ListBox {
                        connect_row_selected[sender] => move |_, row| {
                            sender.input(Msg::Select(row.map(|r| r.index() as usize)));
                        },
                    },
                },
                gtk::Label {
                    set_xalign: 0.0,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                    add_css_class: "dim-label",
                    #[watch]
                    set_visible: !model.progress.is_empty(),
                    #[watch]
                    set_label: &model.progress,
                },
                gtk::Box {
                    set_spacing: 6,
                    #[local_ref]
                    pull_entry -> gtk::Entry {
                        set_hexpand: true,
                        set_placeholder_text: Some("Image to pull, e.g. nginx:latest"),
                        #[watch]
                        set_sensitive: !model.pulling,
                        connect_activate => Msg::Pull,
                    },
                    gtk::Button {
                        set_label: "Pull",
                        #[watch]
                        set_sensitive: !model.pulling,
                        connect_clicked => Msg::Pull,
                    },
                },
            },
            #[wrap(Some)]
            set_end_child = &detail::placeholder(),
        }
    }

    fn init(ctx: AppCtx, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        ctx.on_change(&sender, &[Kind::IMAGE], Cmd::Refresh);
        let model = Self {
            ctx,
            rows: Vec::new(),
            shown: Vec::new(),
            filter: String::new(),
            selected: None,
            pulling: false,
            progress: String::new(),
            list: gtk::ListBox::new(),
            pull_entry: gtk::Entry::new(),
        };
        let list = &model.list;
        let pull_entry = &model.pull_entry;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Msg, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            Msg::Filter(text) => {
                self.filter = text;
                self.render();
            }
            Msg::Select(i) => self.selected = i.and_then(|i| self.shown.get(i)).map(|r| r.target.clone()),
            Msg::Remove => {
                let Some(row) = self.shown.iter().find(|r| Some(&r.target) == self.selected.as_ref()) else {
                    return;
                };
                confirm(
                    Confirm {
                        heading: "Remove image?".into(),
                        items: vec![format!("{}:{} ({}, {})", row.repo, row.tag, row.id, row.size)],
                        action: "Remove".into(),
                        option: Some("Force".into()),
                    },
                    {
                        let target = row.target.clone();
                        move |force| sender.input(Msg::DoRemove { target, force })
                    },
                );
            }
            Msg::DoRemove { target, force } => self.ctx.run(
                &sender,
                "Remove image",
                |d| async move {
                    let opts = RemoveImageOptions { force, ..Default::default() };
                    d.remove_image(&target, Some(opts), None).await
                },
                |_| Cmd::Removed,
            ),
            Msg::Prune => self.ctx.run(
                &sender,
                "Prune images",
                |d| async move {
                    let images = d.list_images(None::<ListImagesOptions>).await?;
                    let opts = ListContainersOptions { all: true, ..Default::default() };
                    let used: Vec<_> =
                        d.list_containers(Some(opts)).await?.into_iter().filter_map(|c| c.image_id).collect();
                    Ok::<_, bollard::errors::Error>(model::unused(&images, &used))
                },
                Cmd::PruneList,
            ),
            Msg::DoPrune => self.ctx.run(
                &sender,
                "Prune images",
                |d| async move {
                    let filters = HashMap::from([("dangling", vec!["false"])]);
                    d.prune_images(Some(PruneImagesOptionsBuilder::new().filters(&filters).build())).await
                },
                Cmd::Pruned,
            ),
            Msg::Pull => self.pull(&sender),
        }
    }

    fn update_cmd(&mut self, cmd: Cmd, sender: ComponentSender<Self>, _root: &Self::Root) {
        match cmd {
            Cmd::Refresh => self.ctx.run(
                &sender,
                "List images",
                |d| async move { d.list_images(None::<ListImagesOptions>).await },
                Cmd::Loaded,
            ),
            Cmd::Loaded(images) => {
                self.rows = model::rows(&images);
                self.render();
            }
            Cmd::PruneList(items) if items.is_empty() => self.ctx.toast("No unused images to prune"),
            Cmd::PruneList(items) => confirm(
                Confirm {
                    heading: format!("Prune {} unused images?", items.len()),
                    items,
                    action: "Prune".into(),
                    option: None,
                },
                move |_| sender.input(Msg::DoPrune),
            ),
            Cmd::Pruned(r) => self.ctx.toast(format!(
                "Pruned {} images, reclaimed {}",
                r.images_deleted.map_or(0, |d| d.len()),
                model::human_size(r.space_reclaimed.unwrap_or(0)),
            )),
            Cmd::Removed => self.ctx.toast("Image removed"),
            Cmd::Progress(line) => self.progress = line,
            Cmd::PullDone(ok) => {
                self.pulling = false;
                if ok {
                    self.pull_entry.set_text("");
                } else {
                    self.progress.clear();
                }
            }
        }
    }
}

impl ImagesView {
    fn render(&mut self) {
        let keep = self.selected.clone();
        self.shown = self.rows.iter().filter(|r| r.matches(&self.filter)).cloned().collect();
        self.list.remove_all();
        for row in &self.shown {
            self.list.append(&row_widget(row));
        }
        if let Some(i) = self.shown.iter().position(|r| Some(&r.target) == keep.as_ref()) {
            self.list.select_row(self.list.row_at_index(i as i32).as_ref());
        }
    }

    fn pull(&mut self, sender: &ComponentSender<Self>) {
        let Some((name, tag)) = model::pull_args(&self.pull_entry.text()) else { return };
        let Some(docker) = self.ctx.docker().cloned() else {
            self.ctx.error("Pull image: Docker is not connected");
            return;
        };
        let sep = if tag.starts_with("sha256:") { '@' } else { ':' };
        let reference = format!("{name}{sep}{tag}");
        let mut progress = PullProgress::new(reference.clone());
        self.pulling = true;
        self.progress = progress.line();
        let ctx = self.ctx.clone();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    let opts = CreateImageOptions { from_image: Some(name), tag: Some(tag), ..Default::default() };
                    let mut stream = docker.create_image(Some(opts), None, None);
                    let mut last = progress.line();
                    let result = loop {
                        let update = match stream.next().await {
                            None => break Ok(()),
                            Some(Err(e)) => Err(e.to_string()),
                            Some(Ok(info)) => progress.update(&info),
                        };
                        if let Err(e) = update {
                            break Err(e);
                        }
                        let line = progress.line();
                        if line != last {
                            out.emit(Cmd::Progress(line.clone()));
                            last = line;
                        }
                    };
                    if let Err(e) = &result {
                        ctx.error(format!("Pull {reference}: {e}"));
                    }
                    out.emit(Cmd::PullDone(result.is_ok()));
                })
                .drop_on_shutdown()
        });
    }
}

fn row_widget(row: &ImageRow) -> gtk::Box {
    let title = gtk::Label::builder()
        .label(format!("{}:{}", row.repo, row.tag))
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    let subtitle = gtk::Label::builder()
        .label(format!("{} · {}", row.id, row.created))
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    let text = gtk::Box::builder().orientation(gtk::Orientation::Vertical).hexpand(true).build();
    text.append(&title);
    text.append(&subtitle);
    let size = gtk::Label::builder().label(&row.size).xalign(1.0).width_chars(7).build();
    let b = gtk::Box::builder().spacing(12).margin_top(6).margin_bottom(6).margin_start(12).margin_end(12).build();
    b.append(&text);
    b.append(&size);
    b
}
