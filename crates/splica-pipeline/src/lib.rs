//! High-level pipeline orchestration connecting demux, decode, filter, encode, and mux.

pub mod event;

pub use event::{PipelineEvent, PipelineEventKind};

/// Builder for configuring and running a media processing pipeline.
///
/// Accepts an optional event callback for structured progress reporting.
/// The callback receives [`PipelineEvent`] values as the pipeline executes.
#[allow(dead_code)] // Fields used once pipeline stages are implemented
pub struct PipelineBuilder<F = fn(PipelineEvent)> {
    on_event: Option<F>,
}

impl PipelineBuilder {
    /// Creates a new pipeline builder with no event callback.
    pub fn new() -> Self {
        Self { on_event: None }
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl<F: Fn(PipelineEvent)> PipelineBuilder<F> {
    /// Sets a callback that receives pipeline events for progress reporting.
    ///
    /// Events include packet reads, frame decodes/encodes, packet writes,
    /// and errors. Each event carries a timestamp and cumulative count.
    ///
    /// # Example
    ///
    /// ```
    /// use splica_pipeline::{PipelineBuilder, PipelineEvent, PipelineEventKind};
    ///
    /// let builder = PipelineBuilder::new().with_event_handler(|event: PipelineEvent| {
    ///     match event.kind {
    ///         PipelineEventKind::PacketsRead { count } => {
    ///             println!("Read {} packets", count);
    ///         }
    ///         _ => {}
    ///     }
    /// });
    /// ```
    pub fn with_event_handler<G: Fn(PipelineEvent)>(self, handler: G) -> PipelineBuilder<G> {
        PipelineBuilder {
            on_event: Some(handler),
        }
    }

    /// Emits an event to the registered handler, if any.
    #[allow(dead_code)] // Used once pipeline stages are implemented
    pub(crate) fn emit(&self, event: PipelineEvent) {
        if let Some(ref f) = self.on_event {
            f(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_that_pipeline_builder_emits_events_to_callback() {
        // GIVEN — a builder with an event collector
        let events: Arc<Mutex<Vec<PipelineEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);

        let builder = PipelineBuilder::new().with_event_handler(move |event: PipelineEvent| {
            events_clone.lock().unwrap().push(event);
        });

        // WHEN — emit a PacketsRead event
        let event = PipelineEvent::new(PipelineEventKind::PacketsRead { count: 42 });
        builder.emit(event);

        // THEN
        let collected = events.lock().unwrap();
        assert_eq!(collected.len(), 1);
        assert!(matches!(
            collected[0].kind,
            PipelineEventKind::PacketsRead { count: 42 }
        ));
    }

    #[test]
    fn test_that_pipeline_builder_without_handler_does_not_panic() {
        // GIVEN — a builder with no event handler
        let builder = PipelineBuilder::new();

        // WHEN — emit an event (should be a no-op)
        let event = PipelineEvent::new(PipelineEventKind::FramesDecoded { count: 10 });
        builder.emit(event);

        // THEN — no panic
    }

    #[test]
    fn test_that_pipeline_event_carries_timestamp() {
        // GIVEN
        let before = std::time::Instant::now();
        let event = PipelineEvent::new(PipelineEventKind::FramesEncoded { count: 5 });
        let after = std::time::Instant::now();

        // THEN
        assert!(event.timestamp >= before);
        assert!(event.timestamp <= after);
    }
}
