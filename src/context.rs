use std::fmt::Display;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bollard::Docker;
use bollard::models::{EventMessage, EventMessageTypeEnum};
use relm4::{Component, ComponentSender, Sender};
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub type Kind = EventMessageTypeEnum;

#[derive(Clone, Debug)]
pub enum DockerEvent {
    Event(EventMessage),
    Resync,
}

impl DockerEvent {
    pub fn concerns(&self, kinds: &[Kind]) -> bool {
        match self {
            DockerEvent::Resync => true,
            DockerEvent::Event(ev) => ev.typ.is_some_and(|t| kinds.contains(&t)),
        }
    }
}

#[derive(Debug)]
pub enum Notice {
    Error(String),
    Toast(String),
}

#[derive(Clone)]
pub struct AppCtx {
    docker: Arc<OnceLock<Docker>>,
    events: broadcast::Sender<DockerEvent>,
    notices: Sender<Notice>,
}

impl AppCtx {
    pub fn new(notices: Sender<Notice>) -> Self {
        let (events, _) = broadcast::channel(256);
        Self { docker: Arc::default(), events, notices }
    }

    pub fn docker(&self) -> Option<&Docker> {
        self.docker.get()
    }

    pub fn set_docker(&self, docker: Docker) -> Docker {
        self.docker.get_or_init(|| docker).clone()
    }

    pub fn publish(&self, event: DockerEvent) {
        let _ = self.events.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DockerEvent> {
        self.events.subscribe()
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.notices.emit(Notice::Error(msg.into()));
    }

    pub fn toast(&self, msg: impl Into<String>) {
        self.notices.emit(Notice::Toast(msg.into()));
    }

    pub fn on_change<C>(&self, sender: &ComponentSender<C>, kinds: &'static [Kind], msg: C::CommandOutput)
    where
        C: Component,
        C::CommandOutput: Clone,
    {
        let mut rx = self.subscribe();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    loop {
                        match rx.recv().await {
                            Ok(ev) if !ev.concerns(kinds) => continue,
                            Err(RecvError::Closed) => return,
                            _ => {}
                        }
                        tokio::time::sleep(DEBOUNCE).await;
                        while !matches!(rx.try_recv(), Err(TryRecvError::Empty | TryRecvError::Closed)) {}
                        if out.send(msg.clone()).is_err() {
                            return;
                        }
                    }
                })
                .drop_on_shutdown()
        });
    }

    pub fn run<C, T, E, Fut>(
        &self,
        sender: &ComponentSender<C>,
        what: &'static str,
        call: impl FnOnce(Docker) -> Fut,
        on_ok: impl FnOnce(T) -> C::CommandOutput + Send + 'static,
    ) where
        C: Component,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: Display,
    {
        let Some(docker) = self.docker() else {
            self.error(format!("{what}: Docker is not connected"));
            return;
        };
        let fut = call(docker.clone());
        let ctx = self.clone();
        sender.command(move |out, shutdown| {
            shutdown
                .register(async move {
                    match fut.await {
                        Ok(v) => out.emit(on_ok(v)),
                        Err(e) => ctx.error(format!("{what}: {e}")),
                    }
                })
                .drop_on_shutdown()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(typ: Option<Kind>) -> DockerEvent {
        DockerEvent::Event(EventMessage { typ, ..Default::default() })
    }

    #[test]
    fn resync_concerns_every_view() {
        assert!(DockerEvent::Resync.concerns(&[Kind::IMAGE]));
        assert!(DockerEvent::Resync.concerns(&[]));
    }

    #[test]
    fn event_concerns_only_listed_kinds() {
        assert!(event(Some(Kind::IMAGE)).concerns(&[Kind::VOLUME, Kind::IMAGE]));
        assert!(!event(Some(Kind::IMAGE)).concerns(&[Kind::CONTAINER]));
    }

    #[test]
    fn untyped_event_concerns_nothing() {
        assert!(!event(None).concerns(&[Kind::CONTAINER]));
    }
}
