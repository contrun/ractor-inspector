use std::mem::ManuallyDrop;

use super::*;

use ractor::{async_trait as rasync_trait, cast, Actor, ActorProcessingErr, ActorRef};

const STOPPING_STATE: u8 = 10;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Message {
    Ping,
    Pong,
}

type RealState = u8;

type ActorState = Box<u8>;

type InspectorState = ManuallyDrop<ActorState>;

type Arguments = ();

type Harness = ActorTestHarness<Message, RealState, ActorState, InspectorState>;

impl Message {
    // retrieve the next message in the sequence
    fn next(&self) -> Self {
        match self {
            Self::Ping => Self::Pong,
            Self::Pong => Self::Ping,
        }
    }
}

pub struct PingPong {
    harness: Option<Harness>,
}

impl ActorWithTestHarness<Message, RealState, ActorState, InspectorState> for PingPong {
    fn with_test_harness(self, harness: Harness) -> Self {
        Self {
            harness: Some(harness),
        }
    }

    fn get_test_harness(&self) -> Option<&Harness> {
        self.harness.as_ref()
    }
}

#[derive(Default)]
pub struct PingPongInspectorPlugin {
    current_message: Option<Message>,
    current_state: Option<RealState>,
    _phantom: std::marker::PhantomData<(RealState, Message)>,
}

impl PingPongInspectorPlugin {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }
}

impl InspectorPlugin for PingPongInspectorPlugin {
    type ActorState = RealState;
    type ActorMessage = Message;

    fn actor_started(&mut self, actor_state: &mut Self::ActorState) {
        self.current_state = Some(*actor_state);
        self.current_message = Some(Message::Ping);
    }

    fn message_handled(&mut self, actor_state: &mut Self::ActorState, message: Self::ActorMessage) {
        // Everytime we handle the message, we increment the state by 1.
        assert_eq!(self.current_state, Some(*actor_state - 1));
        assert_eq!(self.current_message, Some(message));
        self.current_message = Some(message.next());
        self.current_state = Some(*actor_state);
    }

    fn actor_stopped(&mut self, actor_state: &mut Self::ActorState) {
        assert_eq!(*actor_state, STOPPING_STATE);
    }
}

#[rasync_trait]
impl Actor for PingPong {
    type Msg = Message;
    type State = ActorState;
    type Arguments = Arguments;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let myself = self.get_mediator().unwrap_or(myself);

        cast!(myself, Message::Ping)?;

        let state = Box::new(0u8);

        match self.get_test_harness() {
            Some(harness) => Ok(harness.leak_state(state).await),
            None => Ok(state),
        }
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let myself = self.get_mediator().unwrap_or(myself);
        let state = state.as_mut();
        let guard = match self.get_test_harness() {
            Some(harness) => Some((harness, harness.wait_to_handle_message().await)),
            None => None,
        };
        *state += 1;
        if *state < STOPPING_STATE {
            println!("{:?}", &message);
            cast!(myself, message.next())?;
        } else {
            println!("PingPong: Exiting");
            myself.stop(None);
        }
        if let Some((harness, guard)) = guard {
            harness.notify_message_handled(guard).await;
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let (Some(mediator), Some(harness)) = (self.get_mediator(), self.get_test_harness()) {
            println!("PingPong: stopping inspector");
            mediator.stop(Some("sub actor stopped".to_string()));
            let _ = harness.wait_for_lock().await;
            println!("PingPong: inspector stopped");
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_ping_pong_without_harness() {
    let actor = PingPong { harness: None };
    let arguments = ();
    let (_actor, handle) = Actor::spawn(None, actor, arguments)
        .await
        .expect("start actor");
    handle.await.expect("actor should not fail");
}

#[tokio::test]
async fn test_ping_pong_with_noop() {
    let actor = PingPong { harness: None };
    let arguments = ();
    let plugin = InspectorPluginNoop::new();
    let (_actor, handle) = Inspector::start(actor, arguments, plugin)
        .await
        .expect("start inspector");
    handle.await.expect("inspector should not fail");
}

#[tokio::test]
async fn test_ping_pong_with_dumper() {
    let actor = PingPong { harness: None };
    let arguments = ();
    let plugin = InspectorPluginDumper::new();
    let (_actor, handle) = Inspector::start(actor, arguments, plugin)
        .await
        .expect("start inspector");
    handle.await.expect("inspector should not fail");
}

#[tokio::test]
async fn test_ping_pong_with_ping_pong_plugin() {
    let actor = PingPong { harness: None };
    let arguments = ();
    let plugin = PingPongInspectorPlugin::new();
    let (_actor, handle) = Inspector::start(actor, arguments, plugin)
        .await
        .expect("start inspector");
    handle.await.expect("inspector should not fail");
}
