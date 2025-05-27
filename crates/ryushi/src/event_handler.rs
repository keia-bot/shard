use std::convert::Infallible;
use std::fmt::Debug;

use twilight_gateway::CloseFrame;
use twilight_gateway::error::ReceiveMessageError;
use twilight_model::gateway::event::DispatchEvent;

/// A trait that defines methods that are called on internal shard events, such
/// as dispatches, close frames, and errors.
///
/// Implementations of this trait should be designed to handle these events
/// efficiently, as they are called within the event loop of the shard.
pub trait EventHandler: Send {
    type Error: Debug;

    /// Handle a Dispatch event from the shard.
    fn on_dispatch(&self, event: DispatchEvent) -> Result<(), Self::Error>;

    /// Handle a close frame received from the shard.
    fn on_close(&self, frame: Option<CloseFrame<'_>>) -> Result<(), Self::Error>;

    /// Handle an error received from the shard.
    fn on_error(&self, error: ReceiveMessageError) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy)]
pub struct NoopEventHandler;

impl EventHandler for NoopEventHandler {
    type Error = Infallible;

    fn on_dispatch(&self, _event: DispatchEvent) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_close(&self, _frame: Option<CloseFrame<'_>>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn on_error(&self, _error: ReceiveMessageError) -> Result<(), Self::Error> {
        Ok(())
    }
}
