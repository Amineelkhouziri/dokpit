use std::time::Duration;

use bollard::{API_DEFAULT_VERSION, Docker};
use bollard::errors::Error;
use futures_util::StreamExt;
use relm4::Sender;

use crate::context::{AppCtx, DockerEvent};

const RETRY: Duration = Duration::from_secs(3);
const TIMEOUT: u64 = 120;

#[derive(Debug)]
pub enum Status {
    Up,
    Down(String),
}

pub async fn watch(ctx: AppCtx, status: Sender<Status>) {
    loop {
        let reason = match connect(&ctx).await {
            Err(e) => e.to_string(),
            Ok(docker) => {
                status.emit(Status::Up);
                ctx.publish(DockerEvent::Resync);
                let mut events = docker.events(None);
                loop {
                    match events.next().await {
                        Some(Ok(ev)) => ctx.publish(DockerEvent::Event(ev)),
                        Some(Err(e)) => break format!("Event stream: {e}"),
                        None => break "Event stream closed".to_string(),
                    }
                }
            }
        };
        status.emit(Status::Down(reason));
        tokio::time::sleep(RETRY).await;
    }
}

async fn connect(ctx: &AppCtx) -> Result<Docker, Error> {
    let docker = match ctx.docker() {
        Some(docker) => docker.clone(),
        None => ctx.set_docker(Docker::connect_with_unix(ctx.host(), TIMEOUT, API_DEFAULT_VERSION)?),
    };
    docker.ping().await?;
    Ok(docker)
}
