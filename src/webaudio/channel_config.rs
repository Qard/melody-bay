fn node_channel_config(graph: &Arc<Mutex<GraphInner>>, id: NodeId) -> ChannelConfig {
    graph
        .lock()
        .expect("graph mutex poisoned")
        .nodes
        .get(id.0)
        .map(|node| node.channel_config)
        .unwrap_or_default()
}

fn try_set_node_channel_config(
    graph: &Arc<Mutex<GraphInner>>,
    id: NodeId,
    channel_count: usize,
    channel_count_mode: ChannelCountMode,
    channel_interpretation: ChannelInterpretation,
) -> Result<(), GraphError> {
    if !(1..=32).contains(&channel_count) {
        return Err(GraphError::InvalidChannelCount);
    }
    let mut inner = graph.lock().expect("graph mutex poisoned");
    inner.validate_node(id)?;
    validate_channel_config_for_node(
        &inner.nodes[id.0].kind,
        channel_count,
        channel_count_mode,
        channel_interpretation,
    )?;
    inner.nodes[id.0].channel_config = ChannelConfig {
        channel_count,
        channel_count_mode,
        channel_interpretation,
    };
    Ok(())
}

macro_rules! impl_node_channel_config {
    ($node:ty) => {
        impl $node {
            #[must_use]
            pub fn channel_count(&self) -> usize {
                node_channel_config(&self.graph, self.id).channel_count
            }

            #[must_use]
            pub fn channel_count_mode(&self) -> ChannelCountMode {
                node_channel_config(&self.graph, self.id).channel_count_mode
            }

            #[must_use]
            pub fn channel_interpretation(&self) -> ChannelInterpretation {
                node_channel_config(&self.graph, self.id).channel_interpretation
            }

            pub fn try_set_channel_config(
                &self,
                channel_count: usize,
                channel_count_mode: ChannelCountMode,
                channel_interpretation: ChannelInterpretation,
            ) -> Result<(), GraphError> {
                try_set_node_channel_config(
                    &self.graph,
                    self.id,
                    channel_count,
                    channel_count_mode,
                    channel_interpretation,
                )
            }
        }
    };
}

