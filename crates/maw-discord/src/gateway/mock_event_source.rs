use super::GatewayEventSource;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use twilight_model::gateway::event::Event;

type MockEventQueue = Arc<Mutex<Vec<Result<Option<Event>, String>>>>;

/// Shared mock source for offline tests and downstream consumers.
#[derive(Clone, Default)]
pub struct MockGatewaySource {
    events: MockEventQueue,
}

impl MockGatewaySource {
    #[must_use]
    pub fn new(events: Vec<Result<Option<Event>, String>>) -> Self {
        let mut reversed = events;
        reversed.reverse();
        Self {
            events: Arc::new(Mutex::new(reversed)),
        }
    }
}

impl GatewayEventSource for MockGatewaySource {
    fn next_event<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Event>, String>> + Send + 'a>> {
        Box::pin(async move {
            self.events
                .lock()
                .expect("mock source")
                .pop()
                .unwrap_or(Ok(None))
        })
    }
}
