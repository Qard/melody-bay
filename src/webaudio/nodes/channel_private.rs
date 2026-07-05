impl private::AudioNodeHandlePrivate for ChannelSplitterNode {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn context_identity(&self) -> Option<usize> {
        Some(self.context_identity)
    }
}

impl private::AudioNodeHandlePrivate for ChannelMergerNode {
    fn node_id(&self) -> NodeId {
        self.id
    }

    fn context_identity(&self) -> Option<usize> {
        Some(self.context_identity)
    }
}

