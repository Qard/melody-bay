#[allow(private_bounds)]
pub trait AudioNodeHandle: private::AudioNodeHandlePrivate {}

impl<T: private::AudioNodeHandlePrivate + ?Sized> AudioNodeHandle for T {}

impl private::AudioNodeHandlePrivate for NodeId {
    fn node_id(&self) -> NodeId {
        *self
    }
}

impl<T: private::AudioNodeHandlePrivate + ?Sized> private::AudioNodeHandlePrivate for &T {
    fn node_id(&self) -> NodeId {
        (*self).node_id()
    }

    fn context_identity(&self) -> Option<usize> {
        (*self).context_identity()
    }
}

#[derive(Clone)]
pub struct AudioDestinationNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

impl fmt::Debug for AudioDestinationNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("AudioDestinationNode")
            .field(&self.id)
            .finish()
    }
}

impl private::AudioNodeHandlePrivate for AudioDestinationNode {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn context_identity(&self) -> Option<usize> {
        Some(Arc::as_ptr(&self.graph) as usize)
    }
}

impl From<AudioDestinationNode> for NodeId {
    fn from(value: AudioDestinationNode) -> Self {
        value.id
    }
}

impl From<&AudioDestinationNode> for NodeId {
    fn from(value: &AudioDestinationNode) -> Self {
        value.id
    }
}

impl AudioDestinationNode {
    #[must_use]
    pub fn max_channel_count(&self) -> usize {
        node_channel_config(&self.graph, self.id).channel_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ParamId {
    node: NodeId,
    param: ParamKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelCountMode {
    Max,
    ClampedMax,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelInterpretation {
    Speakers,
    Discrete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioNodeInfo {
    pub number_of_inputs: usize,
    pub number_of_outputs: usize,
    pub channel_count: usize,
    pub channel_count_mode: ChannelCountMode,
    pub channel_interpretation: ChannelInterpretation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChannelConfig {
    channel_count: usize,
    channel_count_mode: ChannelCountMode,
    channel_interpretation: ChannelInterpretation,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            channel_count: 2,
            channel_count_mode: ChannelCountMode::Max,
            channel_interpretation: ChannelInterpretation::Speakers,
        }
    }
}

