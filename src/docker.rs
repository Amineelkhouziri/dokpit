use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error;
use futures_util::StreamExt;
use relm4::Sender;

use crate::context::{AppCtx, DockerEvent};

const RETRY: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum Status {
    Up,
    Down(String),
}

pub async fn watch(ctx: AppCtx, status: Sender<Status>) {
    loop {
        match connect(&ctx).await {
            Err(e) => {
                status.emit(Status::Down(e.to_string()));
                tokio::time::sleep(RETRY).await;
            }
            Ok(docker) => {
                status.emit(Status::Up);
                ctx.publish(DockerEvent::Resync);
                let mut events = docker.events(None);
                while let Some(Ok(ev)) = events.next().await {
                    ctx.publish(DockerEvent::Event(ev));
                }
            }
        }
    }
}

async fn connect(ctx: &AppCtx) -> Result<Docker, Error> {
    let docker = match ctx.docker() {
        Some(docker) => docker.clone(),
        None => ctx.set_docker(Docker::connect_with_local_defaults()?),
    };
    docker.ping().await?;
    Ok(docker)
}
