use bollard::models::ContainerSummary;
use relm4::ComponentSender;

use super::{ContainersView, Op, run};
use crate::core;
use crate::ui::confirm::{Confirm, confirm};

fn words(op: Op) -> (&'static str, &'static str, &'static str) {
    match op {
        Op::Start => ("Start", "Start project", "Project started"),
        Op::Stop => ("Stop", "Stop project", "Project stopped"),
        Op::Restart => ("Restart", "Restart project", "Project restarted"),
        Op::Remove => ("Remove", "Remove project containers", "Project containers removed"),
    }
}

pub fn request(view: &ContainersView, op: Op, project: &str, sender: &ComponentSender<ContainersView>) {
    let targets: Vec<&ContainerSummary> = core::project_members(&view.containers, project)
        .filter(|c| match op {
            Op::Start => !core::is_up(c),
            Op::Stop => core::is_up(c),
            Op::Restart | Op::Remove => true,
        })
        .collect();
    let (label, what, done) = words(op);
    if targets.is_empty() {
        view.ctx.toast(format!("{project}: nothing to {}", label.to_lowercase()));
        return;
    }
    let ids: Vec<String> = targets.iter().map(|c| c.id.clone().unwrap_or_default()).collect();
    let names: Vec<String> = targets.iter().map(|c| core::display_name(c)).collect();
    let running = targets.iter().filter(|c| core::is_up(c)).count();
    let (ctx, sender) = (view.ctx.clone(), sender.clone());
    let go = move |force: bool| run(&ctx, &sender, op, ids, force, what, done);
    match op {
        Op::Start | Op::Restart => go(false),
        Op::Stop | Op::Remove => {
            let req = Confirm {
                heading: format!("{label} {} containers of {project}?", names.len()),
                items: names,
                action: label.into(),
                option: (matches!(op, Op::Remove) && running > 0).then(|| format!("Force ({running} running)")),
                destructive: true,
            };
            confirm(req, go);
        }
    }
}
