impl Default for AudioContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct GraphInner {
    nodes: Vec<NodeDef>,
    connections: Vec<NodeConnection>,
    param_connections: Vec<ParamConnection>,
    listener: ListenerState,
    sample_rate: u32,
    latency_hint: Option<AudioContextLatencyHint>,
    current_time: f64,
    state: OfflineAudioContextState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeConnection {
    source: NodeId,
    output: usize,
    destination: NodeId,
    input: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParamConnection {
    source: NodeId,
    output: usize,
    destination: ParamId,
}

impl Default for GraphInner {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            connections: Vec::new(),
            param_connections: Vec::new(),
            listener: ListenerState::default(),
            sample_rate: 44_100,
            latency_hint: None,
            current_time: 0.0,
            state: OfflineAudioContextState::Suspended,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ListenerState {
    position: [ParamTimeline; 3],
    forward: [ParamTimeline; 3],
    up: [ParamTimeline; 3],
}

impl Default for ListenerState {
    fn default() -> Self {
        Self {
            position: [
                ParamTimeline::new(0.0),
                ParamTimeline::new(0.0),
                ParamTimeline::new(0.0),
            ],
            forward: [
                ParamTimeline::new(0.0),
                ParamTimeline::new(0.0),
                ParamTimeline::new(-1.0),
            ],
            up: [
                ParamTimeline::new(0.0),
                ParamTimeline::new(1.0),
                ParamTimeline::new(0.0),
            ],
        }
    }
}

impl ListenerState {
    fn position_value(&self) -> [f32; 3] {
        self.position_at(0.0)
    }

    fn position_at(&self, time: f64) -> [f32; 3] {
        [
            self.position[0].value_at(time),
            self.position[1].value_at(time),
            self.position[2].value_at(time),
        ]
    }

    fn forward_value(&self) -> [f32; 3] {
        self.forward_at(0.0)
    }

    fn forward_at(&self, time: f64) -> [f32; 3] {
        [
            self.forward[0].value_at(time),
            self.forward[1].value_at(time),
            self.forward[2].value_at(time),
        ]
    }

    fn up_value(&self) -> [f32; 3] {
        self.up_at(0.0)
    }

    fn up_at(&self, time: f64) -> [f32; 3] {
        [
            self.up[0].value_at(time),
            self.up[1].value_at(time),
            self.up[2].value_at(time),
        ]
    }

    fn param(&self, param: ParamKind) -> Option<&ParamTimeline> {
        match param {
            ParamKind::PositionX => Some(&self.position[0]),
            ParamKind::PositionY => Some(&self.position[1]),
            ParamKind::PositionZ => Some(&self.position[2]),
            ParamKind::ForwardX => Some(&self.forward[0]),
            ParamKind::ForwardY => Some(&self.forward[1]),
            ParamKind::ForwardZ => Some(&self.forward[2]),
            ParamKind::UpX => Some(&self.up[0]),
            ParamKind::UpY => Some(&self.up[1]),
            ParamKind::UpZ => Some(&self.up[2]),
            _ => None,
        }
    }

    fn param_mut(&mut self, param: ParamKind) -> Option<&mut ParamTimeline> {
        match param {
            ParamKind::PositionX => Some(&mut self.position[0]),
            ParamKind::PositionY => Some(&mut self.position[1]),
            ParamKind::PositionZ => Some(&mut self.position[2]),
            ParamKind::ForwardX => Some(&mut self.forward[0]),
            ParamKind::ForwardY => Some(&mut self.forward[1]),
            ParamKind::ForwardZ => Some(&mut self.forward[2]),
            ParamKind::UpX => Some(&mut self.up[0]),
            ParamKind::UpY => Some(&mut self.up[1]),
            ParamKind::UpZ => Some(&mut self.up[2]),
            _ => None,
        }
    }
}

impl GraphInner {
    fn validate_node(&self, id: NodeId) -> Result<(), GraphError> {
        if id.0 < self.nodes.len() {
            Ok(())
        } else {
            Err(GraphError::UnknownNode)
        }
    }

    fn param(&self, id: ParamId) -> Option<&ParamTimeline> {
        if id.node == LISTENER_PARAM_NODE {
            return self.listener.param(id.param);
        }
        self.nodes
            .get(id.node.0)
            .and_then(|node| node.kind.param(id.param))
    }

    fn param_mut(&mut self, id: ParamId) -> Option<&mut ParamTimeline> {
        if id.node == LISTENER_PARAM_NODE {
            return self.listener.param_mut(id.param);
        }
        self.nodes
            .get_mut(id.node.0)
            .and_then(|node| node.kind.param_mut(id.param))
    }

    fn validate_output_index(&self, source: NodeId, output: usize) -> Result<(), GraphError> {
        self.validate_node(source)?;
        let (_, outputs) = self.nodes[source.0].kind.input_output_count();
        if output < outputs {
            Ok(())
        } else {
            Err(GraphError::InvalidConnectionIndex)
        }
    }

    fn validate_input_index(&self, target: NodeId, input: usize) -> Result<(), GraphError> {
        self.validate_node(target)?;
        let (inputs, _) = self.nodes[target.0].kind.input_output_count();
        if input < inputs {
            Ok(())
        } else {
            Err(GraphError::InvalidConnectionIndex)
        }
    }

    fn connect_nodes(&mut self, source: NodeId, target: NodeId) -> Result<(), GraphError> {
        self.connect_nodes_with_indices(source, 0, target, 0)
    }

    fn connect_nodes_with_indices(
        &mut self,
        source: NodeId,
        output: usize,
        target: NodeId,
        input: usize,
    ) -> Result<(), GraphError> {
        self.validate_output_index(source, output)?;
        self.validate_input_index(target, input)?;
        if (self.has_path(target, source) || source == target)
            && !self.cycle_contains_delay(source, target)
        {
            return Err(GraphError::Cycle);
        }
        let connection = NodeConnection {
            source,
            output,
            destination: target,
            input,
        };
        if !self.connections.contains(&connection) {
            self.connections.push(connection);
        }
        Ok(())
    }

    fn disconnect_nodes(&mut self, source: NodeId, target: NodeId) -> Result<(), GraphError> {
        self.disconnect_nodes_with_indices(source, 0, target, 0)
    }

    fn disconnect_nodes_with_indices(
        &mut self,
        source: NodeId,
        output: usize,
        target: NodeId,
        input: usize,
    ) -> Result<(), GraphError> {
        self.validate_output_index(source, output)?;
        self.validate_input_index(target, input)?;
        self.connections.retain(|connection| {
            *connection
                != NodeConnection {
                    source,
                    output,
                    destination: target,
                    input,
                }
        });
        Ok(())
    }

    fn connect_param_node(&mut self, source: NodeId, target: ParamId) -> Result<(), GraphError> {
        self.connect_param_node_from_output(source, 0, target)
    }

    fn connect_param_node_from_output(
        &mut self,
        source: NodeId,
        output: usize,
        target: ParamId,
    ) -> Result<(), GraphError> {
        self.validate_output_index(source, output)?;
        self.validate_node(target.node)?;
        let connection = ParamConnection {
            source,
            output,
            destination: target,
        };
        if !self.param_connections.contains(&connection) {
            self.param_connections.push(connection);
        }
        Ok(())
    }

    fn disconnect_param_node(&mut self, source: NodeId, target: ParamId) -> Result<(), GraphError> {
        self.disconnect_param_node_from_output(source, 0, target)
    }

    fn disconnect_param_node_from_output(
        &mut self,
        source: NodeId,
        output: usize,
        target: ParamId,
    ) -> Result<(), GraphError> {
        self.validate_output_index(source, output)?;
        self.validate_node(target.node)?;
        self.param_connections.retain(|connection| {
            *connection
                != ParamConnection {
                    source,
                    output,
                    destination: target,
                }
        });
        Ok(())
    }

    fn has_path(&self, from: NodeId, to: NodeId) -> bool {
        let mut queue = VecDeque::from([from]);
        let mut seen = vec![false; self.nodes.len()];
        while let Some(node) = queue.pop_front() {
            if node == to {
                return true;
            }
            if seen[node.0] {
                continue;
            }
            seen[node.0] = true;
            for target in self
                .connections
                .iter()
                .filter(|connection| connection.source == node)
                .map(|connection| connection.destination)
            {
                queue.push_back(target);
            }
            for target in self
                .param_connections
                .iter()
                .filter(|connection| connection.source == node)
                .map(|connection| connection.destination)
            {
                queue.push_back(target.node);
            }
        }
        false
    }

    fn cycle_contains_delay(&self, source: NodeId, target: NodeId) -> bool {
        if self.is_delay_node(source) || self.is_delay_node(target) {
            return true;
        }
        let mut queue = VecDeque::from([target]);
        let mut seen = vec![false; self.nodes.len()];
        while let Some(node) = queue.pop_front() {
            if seen[node.0] {
                continue;
            }
            seen[node.0] = true;
            if self.is_delay_node(node) {
                return true;
            }
            for next in self
                .connections
                .iter()
                .filter(|connection| connection.source == node)
                .map(|connection| connection.destination)
            {
                queue.push_back(next);
            }
            for next in self
                .param_connections
                .iter()
                .filter(|connection| connection.source == node)
                .map(|connection| connection.destination.node)
            {
                if next != LISTENER_PARAM_NODE {
                    queue.push_back(next);
                }
            }
        }
        false
    }

    fn is_delay_node(&self, id: NodeId) -> bool {
        self.nodes
            .get(id.0)
            .is_some_and(|node| matches!(node.kind, NodeKind::Delay { .. }))
    }

    fn compile(&self) -> Result<CompiledGraph, GraphError> {
        let mut indegree = vec![0usize; self.nodes.len()];
        for connection in &self.connections {
            if self.is_delay_node(connection.destination) {
                continue;
            }
            indegree[connection.destination.0] += 1;
        }
        for connection in &self.param_connections {
            indegree[connection.destination.node.0] += 1;
        }
        let mut queue = VecDeque::new();
        for (index, degree) in indegree.iter().enumerate() {
            if *degree == 0 {
                queue.push_back(NodeId(index));
            }
        }
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(node) = queue.pop_front() {
            order.push(node);
            for target in self
                .connections
                .iter()
                .filter(|connection| connection.source == node)
                .map(|connection| connection.destination)
            {
                if self.is_delay_node(target) {
                    continue;
                }
                indegree[target.0] -= 1;
                if indegree[target.0] == 0 {
                    queue.push_back(target);
                }
            }
            for target in self
                .param_connections
                .iter()
                .filter(|connection| connection.source == node)
                .map(|connection| connection.destination)
            {
                indegree[target.node.0] -= 1;
                if indegree[target.node.0] == 0 {
                    queue.push_back(target.node);
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err(GraphError::Cycle);
        }
        let mut inbound_connections = vec![Vec::new(); self.nodes.len()];
        for connection in &self.connections {
            if let Some(inbound) = inbound_connections.get_mut(connection.destination.0) {
                inbound.push(*connection);
            }
        }
        Ok(CompiledGraph {
            nodes: self.nodes.clone(),
            connections: self.connections.clone(),
            inbound_connections,
            param_connections: self.param_connections.clone(),
            delay_cycle_nodes: self.delay_cycle_nodes(),
            order,
            sample_voice: self.compiled_sample_voice(),
            listener: self.listener.clone(),
            sample_rate: self.sample_rate,
        })
    }

    fn compiled_sample_voice(&self) -> Option<CompiledSampleVoice> {
        if !self.param_connections.is_empty()
            || self.nodes.len() != 5
            || self.connections.len() != 4
        {
            return None;
        }

        let source = self.nodes.iter().enumerate().find_map(|(index, node)| {
            matches!(node.kind, NodeKind::AudioBufferSource { .. }).then_some(NodeId(index))
        })?;
        let pan = self.nodes.iter().enumerate().find_map(|(index, node)| {
            matches!(node.kind, NodeKind::StereoPanner { .. }).then_some(NodeId(index))
        })?;
        let gains = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                matches!(node.kind, NodeKind::Gain { .. }).then_some(NodeId(index))
            })
            .collect::<Vec<_>>();
        if gains.len() != 2 {
            return None;
        }

        let destination = NodeId(0);
        if !self
            .connections
            .iter()
            .any(|connection| connection.source == pan && connection.destination == destination)
        {
            return None;
        }
        let channel_gain = gains.iter().copied().find(|gain| {
            self.connections
                .iter()
                .any(|connection| connection.source == *gain && connection.destination == pan)
        })?;
        let envelope_gain = gains.iter().copied().find(|gain| {
            *gain != channel_gain
                && self.connections.iter().any(|connection| {
                    connection.source == *gain && connection.destination == channel_gain
                })
        })?;
        if !self.connections.iter().any(|connection| {
            connection.source == source && connection.destination == envelope_gain
        }) {
            return None;
        }

        Some(CompiledSampleVoice {
            source,
            envelope_gain,
            channel_gain,
            pan,
        })
    }

    fn delay_cycle_nodes(&self) -> Vec<bool> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let delay = NodeId(index);
                matches!(node.kind, NodeKind::Delay { .. })
                    && (self.connections.iter().any(|connection| {
                        connection.source == delay
                            && (connection.destination == delay
                                || self.has_path(connection.destination, delay))
                    }) || self.param_connections.iter().any(|connection| {
                        connection.source == delay
                            && self.has_path(connection.destination.node, delay)
                    }))
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct NodeDef {
    kind: NodeKind,
    channel_config: ChannelConfig,
    label: Option<String>,
}

impl NodeDef {
    fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            channel_config: ChannelConfig::default(),
            label: None,
        }
    }

    fn destination(channel_count: usize) -> Self {
        let mut node = Self::new(NodeKind::Destination { channel_count });
        node.channel_config = ChannelConfig {
            channel_count,
            channel_count_mode: ChannelCountMode::Explicit,
            channel_interpretation: ChannelInterpretation::Speakers,
        };
        node
    }

    fn oscillator(waveform: Waveform) -> Self {
        Self::new(NodeKind::Oscillator {
            waveform,
            frequency: ParamTimeline::new(440.0),
            detune: ParamTimeline::new(0.0)
                .with_nominal_range(-DETUNE_NOMINAL_LIMIT, DETUNE_NOMINAL_LIMIT),
            periodic_wave: None,
            start_time: 0.0,
            stop_time: None,
            start_scheduled: false,
            stop_scheduled: false,
            ended: Arc::new(AtomicBool::new(false)),
        })
    }

    fn constant(value: f32) -> Self {
        Self::new(NodeKind::Constant {
            offset: ParamTimeline::new(value),
            start_time: 0.0,
            stop_time: None,
            start_scheduled: false,
            stop_scheduled: false,
            ended: Arc::new(AtomicBool::new(false)),
        })
    }

    fn gain() -> Self {
        Self::new(NodeKind::Gain {
            gain: ParamTimeline::new(1.0),
        })
    }

    fn fixed_clamped_max(kind: NodeKind) -> Self {
        let mut node = Self::new(kind);
        node.channel_config = ChannelConfig {
            channel_count: 2,
            channel_count_mode: ChannelCountMode::ClampedMax,
            channel_interpretation: ChannelInterpretation::Speakers,
        };
        node
    }

    fn channel_splitter(outputs: usize) -> Self {
        let mut node = Self::new(NodeKind::ChannelSplitter { outputs });
        node.channel_config = ChannelConfig {
            channel_count: outputs,
            channel_count_mode: ChannelCountMode::Explicit,
            channel_interpretation: ChannelInterpretation::Discrete,
        };
        node
    }

    fn channel_merger(inputs: usize) -> Self {
        let mut node = Self::new(NodeKind::ChannelMerger { inputs });
        node.channel_config = ChannelConfig {
            channel_count: 1,
            channel_count_mode: ChannelCountMode::Explicit,
            channel_interpretation: ChannelInterpretation::Speakers,
        };
        node
    }

    fn info(&self) -> AudioNodeInfo {
        let (number_of_inputs, number_of_outputs) = self.kind.input_output_count();
        AudioNodeInfo {
            number_of_inputs,
            number_of_outputs,
            channel_count: self.channel_config.channel_count,
            channel_count_mode: self.channel_config.channel_count_mode,
            channel_interpretation: self.channel_config.channel_interpretation,
        }
    }
}

#[derive(Debug, Clone)]
enum NodeKind {
    Destination {
        channel_count: usize,
    },
    Oscillator {
        waveform: Waveform,
        frequency: ParamTimeline,
        detune: ParamTimeline,
        periodic_wave: Option<PeriodicWave>,
        start_time: f64,
        stop_time: Option<f64>,
        start_scheduled: bool,
        stop_scheduled: bool,
        ended: Arc<AtomicBool>,
    },
    Constant {
        offset: ParamTimeline,
        start_time: f64,
        stop_time: Option<f64>,
        start_scheduled: bool,
        stop_scheduled: bool,
        ended: Arc<AtomicBool>,
    },
    Gain {
        gain: ParamTimeline,
    },
    AudioBufferSource {
        buffer: Option<AudioBuffer>,
        buffer_assigned: bool,
        acquired_buffer: Option<AudioBuffer>,
        playback_rate: ParamTimeline,
        detune: ParamTimeline,
        looping: bool,
        loop_range: Option<(f64, f64)>,
        start_time: f64,
        stop_time: Option<f64>,
        start_scheduled: bool,
        stop_scheduled: bool,
        ended: Arc<AtomicBool>,
        offset: f64,
        duration: Option<f64>,
    },
    ExternalSound {
        data: ExternalSoundDataNode,
        start_time: f64,
        stop_time: Option<f64>,
        start_scheduled: bool,
        stop_scheduled: bool,
        ended: Arc<AtomicBool>,
    },
    StereoPanner {
        pan: ParamTimeline,
    },
    BiquadFilter {
        kind: BiquadFilterType,
        frequency: ParamTimeline,
        detune: ParamTimeline,
        q: ParamTimeline,
        gain: ParamTimeline,
    },
    IirFilter {
        feedforward: Vec<f32>,
        feedback: Vec<f32>,
    },
    Delay {
        delay_time: ParamTimeline,
        max_delay_time: Option<f32>,
    },
    WaveShaper {
        curve: Option<Vec<f32>>,
        oversample: Oversample,
    },
    DynamicsCompressor {
        threshold: ParamTimeline,
        knee: ParamTimeline,
        ratio: ParamTimeline,
        attack: ParamTimeline,
        release: ParamTimeline,
        reduction: Arc<AtomicU32>,
    },
    Convolver {
        buffer: Option<AudioBuffer>,
        normalize: bool,
        buffer_normalize: bool,
    },
    Analyser {
        state: Arc<Mutex<AnalyserState>>,
    },
    Panner {
        position_x: ParamTimeline,
        position_y: ParamTimeline,
        position_z: ParamTimeline,
        orientation_x: ParamTimeline,
        orientation_y: ParamTimeline,
        orientation_z: ParamTimeline,
        panning_model: PanningModel,
        distance_model: DistanceModel,
        ref_distance: f32,
        max_distance: f32,
        rolloff_factor: f32,
        cone_inner_angle: f32,
        cone_outer_angle: f32,
        cone_outer_gain: f32,
    },
    ChannelSplitter {
        outputs: usize,
    },
    ChannelMerger {
        inputs: usize,
    },
    AudioWorklet {
        inputs: usize,
        outputs: usize,
        output_channel_count: Option<Vec<usize>>,
        parameters: Vec<(String, ParamTimeline)>,
        processor_options: HashMap<String, String>,
        processor: AudioWorkletProcessorNode,
    },
}

impl NodeKind {
    fn input_output_count(&self) -> (usize, usize) {
        match self {
            Self::Destination { .. } => (1, 0),
            Self::Oscillator { .. }
            | Self::Constant { .. }
            | Self::AudioBufferSource { .. }
            | Self::ExternalSound { .. } => (0, 1),
            Self::ChannelSplitter { outputs } => (1, *outputs),
            Self::ChannelMerger { inputs } => (*inputs, 1),
            Self::AudioWorklet {
                inputs, outputs, ..
            } => (*inputs, *outputs),
            Self::Gain { .. }
            | Self::StereoPanner { .. }
            | Self::BiquadFilter { .. }
            | Self::IirFilter { .. }
            | Self::Delay { .. }
            | Self::WaveShaper { .. }
            | Self::DynamicsCompressor { .. }
            | Self::Convolver { .. }
            | Self::Analyser { .. }
            | Self::Panner { .. } => (1, 1),
        }
    }

    fn param(&self, param: ParamKind) -> Option<&ParamTimeline> {
        match (self, param) {
            (Self::Gain { gain }, ParamKind::Gain)
            | (
                Self::Oscillator {
                    frequency: gain, ..
                },
                ParamKind::Frequency,
            )
            | (Self::Oscillator { detune: gain, .. }, ParamKind::Detune)
            | (Self::Constant { offset: gain, .. }, ParamKind::Offset)
            | (
                Self::AudioBufferSource {
                    playback_rate: gain,
                    ..
                },
                ParamKind::PlaybackRate,
            )
            | (Self::AudioBufferSource { detune: gain, .. }, ParamKind::Detune)
            | (Self::StereoPanner { pan: gain }, ParamKind::Pan)
            | (
                Self::BiquadFilter {
                    frequency: gain, ..
                },
                ParamKind::Frequency,
            )
            | (Self::BiquadFilter { detune: gain, .. }, ParamKind::Detune)
            | (Self::BiquadFilter { q: gain, .. }, ParamKind::Q)
            | (Self::BiquadFilter { gain, .. }, ParamKind::FilterGain)
            | (
                Self::Delay {
                    delay_time: gain, ..
                },
                ParamKind::DelayTime,
            )
            | (
                Self::DynamicsCompressor {
                    threshold: gain, ..
                },
                ParamKind::Threshold,
            )
            | (Self::DynamicsCompressor { knee: gain, .. }, ParamKind::Knee)
            | (Self::DynamicsCompressor { ratio: gain, .. }, ParamKind::Ratio)
            | (Self::DynamicsCompressor { attack: gain, .. }, ParamKind::Attack)
            | (Self::DynamicsCompressor { release: gain, .. }, ParamKind::Release)
            | (
                Self::Panner {
                    position_x: gain, ..
                },
                ParamKind::PositionX,
            )
            | (
                Self::Panner {
                    position_y: gain, ..
                },
                ParamKind::PositionY,
            )
            | (
                Self::Panner {
                    position_z: gain, ..
                },
                ParamKind::PositionZ,
            )
            | (
                Self::Panner {
                    orientation_x: gain,
                    ..
                },
                ParamKind::OrientationX,
            )
            | (
                Self::Panner {
                    orientation_y: gain,
                    ..
                },
                ParamKind::OrientationY,
            )
            | (
                Self::Panner {
                    orientation_z: gain,
                    ..
                },
                ParamKind::OrientationZ,
            ) => Some(gain),
            (Self::AudioWorklet { parameters, .. }, ParamKind::WorkletParam(index)) => {
                parameters.get(index).map(|(_, param)| param)
            }
            _ => None,
        }
    }

    fn param_mut(&mut self, param: ParamKind) -> Option<&mut ParamTimeline> {
        match (self, param) {
            (Self::Gain { gain }, ParamKind::Gain)
            | (
                Self::Oscillator {
                    frequency: gain, ..
                },
                ParamKind::Frequency,
            )
            | (Self::Oscillator { detune: gain, .. }, ParamKind::Detune)
            | (Self::Constant { offset: gain, .. }, ParamKind::Offset)
            | (
                Self::AudioBufferSource {
                    playback_rate: gain,
                    ..
                },
                ParamKind::PlaybackRate,
            )
            | (Self::AudioBufferSource { detune: gain, .. }, ParamKind::Detune)
            | (Self::StereoPanner { pan: gain }, ParamKind::Pan)
            | (
                Self::BiquadFilter {
                    frequency: gain, ..
                },
                ParamKind::Frequency,
            )
            | (Self::BiquadFilter { detune: gain, .. }, ParamKind::Detune)
            | (Self::BiquadFilter { q: gain, .. }, ParamKind::Q)
            | (Self::BiquadFilter { gain, .. }, ParamKind::FilterGain)
            | (
                Self::Delay {
                    delay_time: gain, ..
                },
                ParamKind::DelayTime,
            )
            | (
                Self::DynamicsCompressor {
                    threshold: gain, ..
                },
                ParamKind::Threshold,
            )
            | (Self::DynamicsCompressor { knee: gain, .. }, ParamKind::Knee)
            | (Self::DynamicsCompressor { ratio: gain, .. }, ParamKind::Ratio)
            | (Self::DynamicsCompressor { attack: gain, .. }, ParamKind::Attack)
            | (Self::DynamicsCompressor { release: gain, .. }, ParamKind::Release)
            | (
                Self::Panner {
                    position_x: gain, ..
                },
                ParamKind::PositionX,
            )
            | (
                Self::Panner {
                    position_y: gain, ..
                },
                ParamKind::PositionY,
            )
            | (
                Self::Panner {
                    position_z: gain, ..
                },
                ParamKind::PositionZ,
            )
            | (
                Self::Panner {
                    orientation_x: gain,
                    ..
                },
                ParamKind::OrientationX,
            )
            | (
                Self::Panner {
                    orientation_y: gain,
                    ..
                },
                ParamKind::OrientationY,
            )
            | (
                Self::Panner {
                    orientation_z: gain,
                    ..
                },
                ParamKind::OrientationZ,
            ) => Some(gain),
            (Self::AudioWorklet { parameters, .. }, ParamKind::WorkletParam(index)) => {
                parameters.get_mut(index).map(|(_, param)| param)
            }
            _ => None,
        }
    }
}

fn validate_channel_config_for_node(
    kind: &NodeKind,
    channel_count: usize,
    channel_count_mode: ChannelCountMode,
    channel_interpretation: ChannelInterpretation,
) -> Result<(), GraphError> {
    match kind {
        NodeKind::Destination {
            channel_count: destination_channel_count,
        } => {
            if channel_count != *destination_channel_count
                || channel_count_mode != ChannelCountMode::Explicit
                || channel_interpretation != ChannelInterpretation::Speakers
            {
                return Err(GraphError::InvalidChannelCount);
            }
        }
        NodeKind::Convolver { .. }
        | NodeKind::DynamicsCompressor { .. }
        | NodeKind::Panner { .. }
        | NodeKind::StereoPanner { .. } => {
            if channel_count > 2 || channel_count_mode == ChannelCountMode::Max {
                return Err(GraphError::InvalidChannelCount);
            }
        }
        NodeKind::ChannelSplitter { outputs } => {
            if channel_count != *outputs {
                return Err(GraphError::InvalidChannelCount);
            }
            if channel_count_mode != ChannelCountMode::Explicit
                || channel_interpretation != ChannelInterpretation::Discrete
            {
                return Err(GraphError::InvalidChannelCount);
            }
        }
        NodeKind::ChannelMerger { .. }
            if channel_count != 1 || channel_count_mode != ChannelCountMode::Explicit =>
        {
            return Err(GraphError::InvalidChannelCount);
        }
        _ => {}
    }
    Ok(())
}

#[derive(Clone)]
struct ExternalSoundDataNode {
    data: Arc<Mutex<Option<Box<dyn ErasedSoundData>>>>,
}

impl ExternalSoundDataNode {
    fn new<D>(data: D) -> Self
    where
        D: SoundData + Send + 'static,
        D::Error: fmt::Debug + Send + Sync + 'static,
    {
        Self {
            data: Arc::new(Mutex::new(Some(Box::new(TypedExternalSoundData {
                data: Some(data),
            })))),
        }
    }

    fn take_sound(&self) -> Result<Box<dyn Sound>, GraphError> {
        let Some(mut data) = self
            .data
            .lock()
            .expect("external sound mutex poisoned")
            .take()
        else {
            return Err(GraphError::ExternalSound(
                "external sound data was already consumed".to_string(),
            ));
        };
        data.take_sound()
    }
}

impl fmt::Debug for ExternalSoundDataNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExternalSoundDataNode")
    }
}

trait ErasedSoundData: Send {
    fn take_sound(&mut self) -> Result<Box<dyn Sound>, GraphError>;
}

struct TypedExternalSoundData<D> {
    data: Option<D>,
}

impl<D> ErasedSoundData for TypedExternalSoundData<D>
where
    D: SoundData + Send + 'static,
    D::Error: fmt::Debug + Send + Sync + 'static,
{
    fn take_sound(&mut self) -> Result<Box<dyn Sound>, GraphError> {
        let Some(data) = self.data.take() else {
            return Err(GraphError::ExternalSound(
                "external sound data was already consumed".to_string(),
            ));
        };
        let (sound, _) = data
            .into_sound()
            .map_err(|error| GraphError::ExternalSound(format!("{error:?}")))?;
        Ok(sound)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    UnknownNode,
    UnknownParam,
    WrongContext,
    InvalidChannel,
    InvalidChannelCount,
    InvalidAudioBuffer,
    InvalidNodeLabel,
    InvalidConnectionIndex,
    InvalidFrequencyResponse,
    InvalidIirFilter,
    InvalidPeriodicWave,
    InvalidWaveShaperCurve,
    InvalidConvolverBuffer,
    InvalidAnalyserConfig,
    InvalidDelayTime,
    InvalidPannerConfig,
    UnsupportedPanningModel,
    InvalidLoopRange,
    InvalidAudioWorkletOptions,
    Cycle,
    ContextClosed,
    InvalidState,
    NegativeTime,
    SourceAlreadyStarted,
    SourceNotStarted,
    SourceAlreadyStopped,
    StopBeforeStart,
    InvalidAutomationValue,
    InvalidAutomationRate,
    ExternalSound(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode => f.write_str("unknown graph node"),
            Self::UnknownParam => f.write_str("unknown graph parameter"),
            Self::WrongContext => f.write_str("node belongs to a different audio context"),
            Self::InvalidChannel => f.write_str("invalid audio buffer channel"),
            Self::InvalidChannelCount => f.write_str("invalid audio node channel count"),
            Self::InvalidAudioBuffer => f.write_str("invalid audio buffer arguments"),
            Self::InvalidNodeLabel => f.write_str("invalid audio node label"),
            Self::InvalidConnectionIndex => f.write_str("invalid audio connection index"),
            Self::InvalidFrequencyResponse => {
                f.write_str("invalid frequency response buffer lengths")
            }
            Self::InvalidIirFilter => f.write_str("invalid IIR filter coefficients"),
            Self::InvalidPeriodicWave => f.write_str("invalid periodic wave coefficients"),
            Self::InvalidWaveShaperCurve => f.write_str("invalid wave shaper curve"),
            Self::InvalidConvolverBuffer => f.write_str("invalid convolver buffer"),
            Self::InvalidAnalyserConfig => f.write_str("invalid analyser configuration"),
            Self::InvalidDelayTime => f.write_str("invalid delay max delay time"),
            Self::InvalidPannerConfig => f.write_str("invalid panner configuration"),
            Self::UnsupportedPanningModel => {
                f.write_str("unsupported panning model: HRTF is not implemented")
            }
            Self::InvalidLoopRange => f.write_str("invalid audio buffer source loop range"),
            Self::InvalidAudioWorkletOptions => f.write_str("invalid audio worklet options"),
            Self::Cycle => f.write_str("graph connection would create a cycle"),
            Self::ContextClosed => f.write_str("audio context is closed"),
            Self::InvalidState => f.write_str("invalid audio context state"),
            Self::NegativeTime => f.write_str("scheduled source time cannot be negative"),
            Self::SourceAlreadyStarted => f.write_str("source node was already started"),
            Self::SourceNotStarted => f.write_str("source node has not been started"),
            Self::SourceAlreadyStopped => f.write_str("source node was already stopped"),
            Self::StopBeforeStart => f.write_str("source stop time is before start time"),
            Self::InvalidAutomationValue => f.write_str("invalid audio parameter automation value"),
            Self::InvalidAutomationRate => f.write_str("invalid audio parameter automation rate"),
            Self::ExternalSound(error) => write!(f, "external sound source failed: {error}"),
        }
    }
}

impl std::error::Error for GraphError {}
