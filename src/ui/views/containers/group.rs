use bollard::models::ContainerSummary;
use bollard::query_parameters::{
    RemoveContainerOptions, RestartContainerOptions, StartContainerOptions, StopContainerOptions,
};
use futures_util::future::try_join_all;
use relm4::ComponentSender;

use super::{Cmd, ContainersView, rows};
use crate::core;
use crate::ui::confirm::{Confirm, confirm};

#[derive(Debug, Clone, Copy)]
pub enum Op {
    Start,
    Stop,
    Restart,
    Remove,
}

impl Op {
    fn words(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Op::Start => ("Start", "Start project", "Project started"),
            Op::Stop => ("Stop", "Stop project", "Project stopped"),
            Op::Restart => ("Restart", "Restart project", "Project restarted"),
            Op::Remove => ("Remove", "Remove project containers", "Project containers removed"),
        }
    }
}

pub fn request(view: &ContainersView, op: Op, project: &str, sender: &ComponentSender<ContainersView>) {
    let Some(group) = core::group_containers(&view.containers)
        .into_iter()
        .find(|g| g.project.as_deref() == Some(project))
    else {
        return;
    };
    let targets: Vec<&ContainerSummary> = group
        .containers
        .iter()
        .filter(|c| match op {
            Op::Start => !rows::is_up(c),
            Op::Stop => rows::is_up(c),
            Op::Restart | Op::Remove => true,
        })
        .collect();
    let (label, what, done) = op.words();
    if targets.is_empty() {
        view.ctx.toast(format!("{project}: nothing to {}", label.to_lowercase()));
        return;
    }
    let ids: Vec<String> = targets.iter().map(|c| c.id.clone().unwrap_or_default()).collect();
    let names: Vec<String> = targets.iter().map(|c| core::display_name(c)).collect();
    let running = targets.iter().filter(|c| rows::is_up(c)).count();
    let (ctx, sender) = (view.ctx.clone(), sender.clone());
    let run = move |force: bool| {
        ctx.run(
            &sender,
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
    };
    match op {
        Op::Start | Op::Restart => run(false),
        Op::Stop | Op::Remove => {
            let req = Confirm {
                heading: format!("{label} {} containers of {project}?", names.len()),
                items: names,
                action: label.into(),
                option: (matches!(op, Op::Remove) && running > 0).then(|| format!("Force ({running} running)")),
            };
            confirm(req, run);
        }
    }
}
