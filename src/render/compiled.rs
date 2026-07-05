#[derive(Debug, Clone)]
struct CompiledGraph {
    nodes: Vec<NodeDef>,
    connections: Vec<NodeConnection>,
    inbound_connections: Vec<Vec<NodeConnection>>,
    param_connections: Vec<ParamConnection>,
    delay_cycle_nodes: Vec<bool>,
    order: Vec<NodeId>,
    sample_voice: Option<CompiledSampleVoice>,
    listener: ListenerState,
    sample_rate: u32,
}

#[derive(Debug, Clone, Copy)]
struct CompiledSampleVoice {
    source: NodeId,
    envelope_gain: NodeId,
    channel_gain: NodeId,
    pan: NodeId,
}

#[derive(Debug, Clone, Copy)]
struct RenderQuantum {
    start: f64,
    global_start: f64,
    sample_dt: f64,
    frames: usize,
    commit_source_state: bool,
}

#[derive(Debug, Clone, Copy)]
struct RenderSample {
    time: f64,
    global_time: f64,
    sample_dt: f64,
    commit_source_state: bool,
}

struct LookaheadFrame<'a, 'stack> {
    time: f64,
    sample_dt: f64,
    frame_offset: usize,
    outputs: &'a [AudioBus],
    node_runtime: &'a [NodeRuntime],
    stack: &'stack mut Vec<NodeId>,
}

struct LookaheadBiquad<'a> {
    node: NodeId,
    kind: BiquadFilterType,
    frequency: &'a ParamTimeline,
    detune: &'a ParamTimeline,
    q: &'a ParamTimeline,
    gain: &'a ParamTimeline,
}

struct LookaheadIir<'a> {
    node: NodeId,
    feedforward: &'a [f32],
    feedback: &'a [f32],
}

struct LookaheadDelay<'a> {
    node: NodeId,
    delay_time: &'a ParamTimeline,
    max_delay_time: Option<f32>,
}

struct LookaheadConvolver<'a> {
    node: NodeId,
    buffer: Option<&'a AudioBuffer>,
    normalize: bool,
}

struct LookaheadDynamics<'a> {
    node: NodeId,
    threshold: &'a ParamTimeline,
    knee: &'a ParamTimeline,
    ratio: &'a ParamTimeline,
    attack: &'a ParamTimeline,
    release: &'a ParamTimeline,
}

struct LookaheadBufferSource<'a> {
    node: NodeId,
    buffer: &'a Option<AudioBuffer>,
    acquired_buffer: &'a Option<AudioBuffer>,
    playback_rate: &'a ParamTimeline,
    detune: &'a ParamTimeline,
    looping: bool,
    loop_range: Option<(f64, f64)>,
    start_time: f64,
    stop_time: Option<f64>,
    start_scheduled: bool,
    offset: f64,
    duration: Option<f64>,
}

struct LookaheadOscillator<'a> {
    node: NodeId,
    waveform: Waveform,
    periodic_wave: Option<&'a PeriodicWave>,
    frequency: &'a ParamTimeline,
    detune: &'a ParamTimeline,
    start_time: f64,
    stop_time: Option<f64>,
    start_scheduled: bool,
}

impl CompiledGraph {
    fn set_destination_channel_count(&mut self, channel_count: usize) {
        let channel_count = channel_count.max(1);
        let Some(destination) = self.nodes.get_mut(0) else {
            return;
        };
        destination.kind = NodeKind::Destination { channel_count };
        destination.channel_config = ChannelConfig {
            channel_count,
            channel_count_mode: ChannelCountMode::Explicit,
            channel_interpretation: ChannelInterpretation::Speakers,
        };
    }
}

enum NodeRuntime {
    None,
    Destination {
        k_rate_quantum_start: Option<f64>,
        k_rate_outputs: Vec<AudioBus>,
        render_quantum_start: Option<f64>,
        current_quantum_outputs: Vec<Vec<AudioBus>>,
        previous_quantum_outputs: Vec<Vec<AudioBus>>,
    },
    Oscillator {
        phase: f64,
    },
    AudioBufferSource {
        source_time: Option<f64>,
    },
    ExternalSound {
        sound: Option<Box<dyn Sound>>,
    },
    Delay {
        buffer: Vec<AudioBus>,
        write_index: usize,
        delay_samples: f64,
    },
    Biquad {
        state: BiquadState,
    },
    Iir {
        x_history: Vec<AudioBus>,
        y_history: Vec<AudioBus>,
    },
    Convolver {
        history: Vec<AudioBus>,
    },
    WaveShaper {
        previous_input: Option<AudioBus>,
    },
    DynamicsCompressor {
        gain_reduction_db: f32,
        pre_delay: VecDeque<(AudioBus, f32)>,
    },
    AudioWorklet {
        quantum_start: Option<f64>,
        quantum_outputs: Vec<AudioBus>,
    },
}

include!("bus.rs");

impl fmt::Debug for NodeRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Destination {
                k_rate_quantum_start,
                k_rate_outputs,
                render_quantum_start,
                current_quantum_outputs,
                previous_quantum_outputs,
            } => f
                .debug_struct("Destination")
                .field("k_rate_quantum_start", k_rate_quantum_start)
                .field("k_rate_outputs_len", &k_rate_outputs.len())
                .field("render_quantum_start", render_quantum_start)
                .field(
                    "current_quantum_outputs_len",
                    &current_quantum_outputs.len(),
                )
                .field(
                    "previous_quantum_outputs_len",
                    &previous_quantum_outputs.len(),
                )
                .finish(),
            Self::Oscillator { phase } => {
                f.debug_struct("Oscillator").field("phase", phase).finish()
            }
            Self::AudioBufferSource { source_time } => f
                .debug_struct("AudioBufferSource")
                .field("source_time", source_time)
                .finish(),
            Self::ExternalSound { sound } => f
                .debug_struct("ExternalSound")
                .field("initialized", &sound.is_some())
                .finish(),
            Self::Delay {
                buffer,
                write_index,
                ..
            } => f
                .debug_struct("Delay")
                .field("buffer_len", &buffer.len())
                .field("write_index", write_index)
                .finish(),
            Self::Biquad { state } => f.debug_struct("Biquad").field("state", state).finish(),
            Self::Iir {
                x_history,
                y_history,
            } => f
                .debug_struct("Iir")
                .field("x_history_len", &x_history.len())
                .field("y_history_len", &y_history.len())
                .finish(),
            Self::Convolver { history } => f
                .debug_struct("Convolver")
                .field("history_len", &history.len())
                .finish(),
            Self::WaveShaper { previous_input } => f
                .debug_struct("WaveShaper")
                .field("has_previous_input", &previous_input.is_some())
                .finish(),
            Self::DynamicsCompressor {
                gain_reduction_db,
                pre_delay,
            } => f
                .debug_struct("DynamicsCompressor")
                .field("gain_reduction_db", gain_reduction_db)
                .field("pre_delay_len", &pre_delay.len())
                .finish(),
            Self::AudioWorklet {
                quantum_start,
                quantum_outputs,
            } => f
                .debug_struct("AudioWorklet")
                .field("quantum_start", quantum_start)
                .field("quantum_outputs_len", &quantum_outputs.len())
                .finish(),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct BiquadState {
    x1: AudioBus,
    x2: AudioBus,
    y1: AudioBus,
    y2: AudioBus,
}

impl BiquadState {
    fn process(&mut self, input: AudioBus, coefficients: BiquadCoefficients) -> AudioBus {
        let channels = input.channels.len().max(1);
        self.resize(channels);
        let mut output = AudioBus::silent(channels);
        output.channels.fill(0.0);
        for channel in 0..channels {
            output.channels[channel] = input.channel(channel) * coefficients.b0
                + self.x1.channel(channel) * coefficients.b1
                + self.x2.channel(channel) * coefficients.b2
                - self.y1.channel(channel) * coefficients.a1
                - self.y2.channel(channel) * coefficients.a2;
        }
        self.x2 = self.x1.clone();
        self.x1 = input;
        self.y2 = self.y1.clone();
        self.y1 = output.clone();
        output
    }

    fn resize(&mut self, channels: usize) {
        resize_bus(&mut self.x1, channels);
        resize_bus(&mut self.x2, channels);
        resize_bus(&mut self.y1, channels);
        resize_bus(&mut self.y2, channels);
    }
}

fn resize_bus(bus: &mut AudioBus, channels: usize) {
    if bus.channels.len() != channels {
        bus.channels.resize(channels, 0.0);
    }
}

#[derive(Debug, Clone, Copy)]
struct BiquadCoefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoefficients {
    fn new(kind: BiquadFilterType, frequency: f32, q: f32, gain_db: f32, sample_rate: f64) -> Self {
        let nyquist = (sample_rate as f32 * 0.5).max(1.0);
        let frequency = frequency.clamp(1.0, nyquist * 0.999);
        let q = q.max(0.0001);
        let omega = TAU * frequency / sample_rate as f32;
        let sin = omega.sin();
        let cos = omega.cos();
        let alpha = sin / (2.0 * q);
        let a = 10.0f32.powf(gain_db / 40.0);
        let sqrt_a = a.sqrt();
        let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;

        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadFilterType::Lowpass => (
                (1.0 - cos) * 0.5,
                1.0 - cos,
                (1.0 - cos) * 0.5,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadFilterType::Highpass => (
                (1.0 + cos) * 0.5,
                -(1.0 + cos),
                (1.0 + cos) * 0.5,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            BiquadFilterType::Bandpass => {
                (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            BiquadFilterType::Lowshelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha),
                (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha,
            ),
            BiquadFilterType::Highshelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha),
                (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha,
            ),
            BiquadFilterType::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            BiquadFilterType::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadFilterType::Allpass => (
                1.0 - alpha,
                -2.0 * cos,
                1.0 + alpha,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };

        let a0 = a0.max(f32::EPSILON);
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn frequency_response(self, frequency_hz: f32, sample_rate: f32) -> (f32, f32) {
        let nyquist = (sample_rate * 0.5).max(1.0);
        if !valid_frequency_response_input(frequency_hz, nyquist) {
            return (f32::NAN, f32::NAN);
        }
        let omega = TAU * frequency_hz / sample_rate.max(1.0);
        let z1_re = omega.cos();
        let z1_im = -omega.sin();
        let z2_re = (2.0 * omega).cos();
        let z2_im = -(2.0 * omega).sin();

        let numerator_re = self.b0 + self.b1 * z1_re + self.b2 * z2_re;
        let numerator_im = self.b1 * z1_im + self.b2 * z2_im;
        let denominator_re = 1.0 + self.a1 * z1_re + self.a2 * z2_re;
        let denominator_im = self.a1 * z1_im + self.a2 * z2_im;

        let denominator_mag_squared =
            denominator_re.mul_add(denominator_re, denominator_im * denominator_im);
        if denominator_mag_squared <= f32::EPSILON {
            return (0.0, 0.0);
        }

        let response_re = (numerator_re * denominator_re + numerator_im * denominator_im)
            / denominator_mag_squared;
        let response_im = (numerator_im * denominator_re - numerator_re * denominator_im)
            / denominator_mag_squared;
        let magnitude = response_re
            .mul_add(response_re, response_im * response_im)
            .sqrt();
        let phase = response_im.atan2(response_re);
        (magnitude, phase)
    }
}

impl CompiledGraph {
    fn runtime(&self) -> Result<Vec<NodeRuntime>, GraphError> {
        self.nodes
            .iter()
            .map(|node| {
                Ok(match &node.kind {
                    NodeKind::Destination { .. } => NodeRuntime::Destination {
                        k_rate_quantum_start: None,
                        k_rate_outputs: Vec::new(),
                        render_quantum_start: None,
                        current_quantum_outputs: Vec::new(),
                        previous_quantum_outputs: Vec::new(),
                    },
                    NodeKind::Oscillator { .. } => NodeRuntime::Oscillator { phase: 0.0 },
                    NodeKind::AudioBufferSource { .. } => {
                        NodeRuntime::AudioBufferSource { source_time: None }
                    }
                    NodeKind::Delay { .. } => NodeRuntime::Delay {
                        buffer: Vec::new(),
                        write_index: 0,
                        delay_samples: 0.0,
                    },
                    NodeKind::ExternalSound { data, .. } => NodeRuntime::ExternalSound {
                        sound: Some(data.take_sound()?),
                    },
                    NodeKind::BiquadFilter { .. } => NodeRuntime::Biquad {
                        state: BiquadState::default(),
                    },
                    NodeKind::IirFilter { .. } => NodeRuntime::Iir {
                        x_history: Vec::new(),
                        y_history: Vec::new(),
                    },
                    NodeKind::Convolver { buffer, .. } => NodeRuntime::Convolver {
                        history: vec![
                            AudioBus::silent(1);
                            buffer.as_ref().map_or(0, AudioBuffer::len)
                        ],
                    },
                    NodeKind::WaveShaper { .. } => NodeRuntime::WaveShaper {
                        previous_input: None,
                    },
                    NodeKind::DynamicsCompressor { .. } => NodeRuntime::DynamicsCompressor {
                        gain_reduction_db: 0.0,
                        pre_delay: VecDeque::new(),
                    },
                    NodeKind::AudioWorklet { .. } => NodeRuntime::AudioWorklet {
                        quantum_start: None,
                        quantum_outputs: Vec::new(),
                    },
                    _ => NodeRuntime::None,
                })
            })
            .collect()
    }

    fn empty_runtime(&self) -> Vec<NodeRuntime> {
        self.nodes
            .iter()
            .map(|node| match &node.kind {
                NodeKind::Destination { .. } => NodeRuntime::Destination {
                    k_rate_quantum_start: None,
                    k_rate_outputs: Vec::new(),
                    render_quantum_start: None,
                    current_quantum_outputs: Vec::new(),
                    previous_quantum_outputs: Vec::new(),
                },
                NodeKind::Oscillator { .. } => NodeRuntime::Oscillator { phase: 0.0 },
                NodeKind::AudioBufferSource { .. } => {
                    NodeRuntime::AudioBufferSource { source_time: None }
                }
                NodeKind::Delay { .. } => NodeRuntime::Delay {
                    buffer: Vec::new(),
                    write_index: 0,
                    delay_samples: 0.0,
                },
                NodeKind::ExternalSound { .. } => NodeRuntime::ExternalSound { sound: None },
                NodeKind::BiquadFilter { .. } => NodeRuntime::Biquad {
                    state: BiquadState::default(),
                },
                NodeKind::IirFilter { .. } => NodeRuntime::Iir {
                    x_history: Vec::new(),
                    y_history: Vec::new(),
                },
                NodeKind::Convolver { buffer, .. } => NodeRuntime::Convolver {
                    history: vec![AudioBus::silent(1); buffer.as_ref().map_or(0, AudioBuffer::len)],
                },
                NodeKind::WaveShaper { .. } => NodeRuntime::WaveShaper {
                    previous_input: None,
                },
                NodeKind::DynamicsCompressor { .. } => NodeRuntime::DynamicsCompressor {
                    gain_reduction_db: 0.0,
                    pre_delay: VecDeque::new(),
                },
                NodeKind::AudioWorklet { .. } => NodeRuntime::AudioWorklet {
                    quantum_start: None,
                    quantum_outputs: Vec::new(),
                },
                _ => NodeRuntime::None,
            })
            .collect()
    }
}

impl CompiledGraph {
    fn render_bus_quantum_with_runtime(
        &self,
        quantum: RenderQuantum,
        note: Option<&NoteEvent>,
        node_runtime: &mut [NodeRuntime],
        info: &Info,
    ) -> Vec<AudioBus> {
        let frames = quantum.frames.min(RENDER_QUANTUM_SIZE_USIZE);
        let mut rendered = Vec::with_capacity(frames);
        let mut outputs = vec![AudioBus::silent(1); self.nodes.len()];
        for frame in 0..frames {
            ensure_graph_outputs(&mut outputs, self.nodes.len());
            rendered.push(self.render_bus_into(
                RenderSample {
                    time: quantum.start + frame as f64 * quantum.sample_dt,
                    global_time: quantum.global_start + frame as f64 * quantum.sample_dt,
                    sample_dt: quantum.sample_dt,
                    commit_source_state: quantum.commit_source_state,
                },
                note,
                node_runtime,
                &mut outputs,
                info,
            ));
        }
        rendered
    }

    fn render_quantum_with_runtime(
        &self,
        quantum: RenderQuantum,
        note: Option<&NoteEvent>,
        node_runtime: &mut [NodeRuntime],
        info: &Info,
    ) -> Vec<Frame> {
        self.render_bus_quantum_with_runtime(quantum, note, node_runtime, info)
            .into_iter()
            .map(|bus| bus.to_frame())
            .collect()
    }

    fn render_note_quantum(
        &self,
        note: &NoteEvent,
        sequence_quantum_start: f64,
        sample_dt: f64,
        frames: usize,
        runtime: &mut VoiceRuntime,
        info: &Info,
    ) -> Vec<Frame> {
        let frames = frames.min(RENDER_QUANTUM_SIZE_USIZE);
        let applies_note_gain = true;
        let mut rendered = if let Some(sample_voice) = self.sample_voice {
            (0..frames)
                .map(|frame| {
                    let sequence_time = sequence_quantum_start + frame as f64 * sample_dt;
                    let local_time = sequence_time - note.start.as_seconds();
                    self.render_sample_voice(
                        sample_voice,
                        note,
                        local_time,
                        sequence_time,
                        sample_dt,
                        runtime,
                    )
                })
                .collect::<Vec<_>>()
        } else {
            let mut node_runtime = std::mem::take(&mut runtime.graph_nodes);
            if node_runtime.len() != self.nodes.len() {
                node_runtime = self.empty_runtime();
            }
            let local_quantum_start = sequence_quantum_start - note.start.as_seconds();
            let frame = self.render_quantum_with_runtime(
                RenderQuantum {
                    start: local_quantum_start,
                    global_start: sequence_quantum_start,
                    sample_dt,
                    frames,
                    commit_source_state: true,
                },
                Some(note),
                &mut node_runtime,
                info,
            );
            runtime.graph_nodes = node_runtime;
            frame
        };
        if applies_note_gain {
            for (frame_index, frame) in rendered.iter_mut().enumerate() {
                let sequence_time = sequence_quantum_start + frame_index as f64 * sample_dt;
                let local_time = sequence_time - note.start.as_seconds();
                *frame *= note.velocity * note.gate_gain(local_time);
            }
        }
        rendered
    }

    fn render_sample_voice(
        &self,
        sample_voice: CompiledSampleVoice,
        note: &NoteEvent,
        local_time: f64,
        sequence_time: f64,
        sample_dt: f64,
        runtime: &mut VoiceRuntime,
    ) -> Frame {
        let NodeKind::AudioBufferSource {
            buffer,
            acquired_buffer,
            playback_rate,
            detune,
            looping,
            loop_range,
            start_time,
            stop_time,
            start_scheduled,
            offset,
            duration,
            ..
        } = &self.nodes[sample_voice.source.0].kind
        else {
            return Frame::ZERO;
        };
        let render_buffer = if *start_scheduled {
            acquired_buffer.as_ref()
        } else {
            buffer.as_ref()
        };
        let Some(buffer) = render_buffer else {
            return Frame::ZERO;
        };
        let start_time = if *start_scheduled { *start_time } else { 0.0 };
        if source_has_ended(local_time, *stop_time)
            || !source_is_active(local_time, start_time, *stop_time)
        {
            return Frame::ZERO;
        }

        let mut playback_rate = timeline_value_for_render_cached(
            playback_rate,
            local_time,
            sequence_time,
            sample_dt,
            &mut runtime.sample_voice_params.playback_rate,
        );
        let detune = timeline_value_for_render_cached(
            detune,
            local_time,
            sequence_time,
            sample_dt,
            &mut runtime.sample_voice_params.detune,
        );
        playback_rate *= 2.0f32.powf(detune / 1200.0);
        playback_rate *= note.pitch_ratio();
        if !playback_rate.is_finite() {
            return Frame::ZERO;
        }

        let source_time = runtime.sample_source_time.get_or_insert_with(|| {
            *offset + (local_time - start_time).max(0.0) * playback_rate as f64
        });
        if buffer_source_duration_elapsed(*source_time, *offset, *duration) {
            return Frame::ZERO;
        }
        if !*looping && buffer_source_time_out_of_bounds(*source_time, buffer.duration()) {
            return Frame::ZERO;
        }

        let mut render_source_time = *source_time;
        let mut effective_loop = None;
        if *looping {
            let (loop_start, loop_end) =
                effective_loop_range(*loop_range, buffer.duration() as f64);
            render_source_time =
                wrap_loop_source_time(render_source_time, loop_start, loop_end, playback_rate);
            effective_loop = Some((loop_start, loop_end));
        }

        let sample = if playback_rate.abs() >= 8.0 {
            let next_source_time = render_source_time + sample_dt * playback_rate as f64;
            sample_voice_frame_between(buffer, render_source_time, next_source_time, effective_loop)
        } else if let Some((loop_start, loop_end)) = effective_loop {
            sample_voice_frame_at_looping(buffer, render_source_time, loop_start, loop_end)
        } else {
            sample_voice_frame_at(buffer, render_source_time)
        };
        *source_time += sample_dt * playback_rate as f64;

        let NodeKind::Gain {
            gain: envelope_gain,
        } = &self.nodes[sample_voice.envelope_gain.0].kind
        else {
            return Frame::ZERO;
        };
        let NodeKind::Gain { gain: channel_gain } = &self.nodes[sample_voice.channel_gain.0].kind
        else {
            return Frame::ZERO;
        };
        let NodeKind::StereoPanner { pan } = &self.nodes[sample_voice.pan.0].kind else {
            return Frame::ZERO;
        };

        let envelope_gain = timeline_value_for_render_cached(
            envelope_gain,
            local_time,
            sequence_time,
            sample_dt,
            &mut runtime.sample_voice_params.envelope_gain,
        );
        let channel_gain = timeline_value_for_render_cached(
            channel_gain,
            local_time,
            sequence_time,
            sample_dt,
            &mut runtime.sample_voice_params.channel_gain,
        );
        let pan = timeline_value_for_render_cached(
            pan,
            local_time,
            sequence_time,
            sample_dt,
            &mut runtime.sample_voice_params.pan,
        );
        pan_sample_voice_frame(sample, envelope_gain * channel_gain, pan)
    }

    #[allow(dead_code)]
    fn render_with_runtime(
        &self,
        time: f64,
        global_time: f64,
        sample_dt: f64,
        note: Option<&NoteEvent>,
        node_runtime: &mut [NodeRuntime],
        info: &Info,
    ) -> Frame {
        self.render_bus_with_runtime(
            RenderSample {
                time,
                global_time,
                sample_dt,
                commit_source_state: true,
            },
            note,
            node_runtime,
            info,
        )
            .to_frame()
    }

    #[allow(dead_code)]
    fn render_bus_with_runtime(
        &self,
        sample: RenderSample,
        note: Option<&NoteEvent>,
        node_runtime: &mut [NodeRuntime],
        info: &Info,
    ) -> AudioBus {
        let mut outputs = vec![AudioBus::silent(1); self.nodes.len()];
        self.render_bus_into(sample, note, node_runtime, &mut outputs, info)
    }

    fn render_bus_into(
        &self,
        sample: RenderSample,
        note: Option<&NoteEvent>,
        node_runtime: &mut [NodeRuntime],
        outputs: &mut [AudioBus],
        info: &Info,
    ) -> AudioBus {
        let sample_context = sample;
        let RenderSample {
            time,
            global_time,
            sample_dt,
            commit_source_state,
        } = sample;
        let quantum_start = k_rate_quantum_start(time, sample_dt);
        let frame_offset = if sample_dt.is_finite() && sample_dt > 0.0 {
            ((time - quantum_start) / sample_dt).round().max(0.0) as usize
        } else {
            0
        };
        if let Some(NodeRuntime::Destination {
            render_quantum_start,
            current_quantum_outputs,
            previous_quantum_outputs,
            ..
        }) = node_runtime.first_mut()
            && render_quantum_start.is_none_or(|start| (start - quantum_start).abs() > f64::EPSILON)
            {
                *previous_quantum_outputs = std::mem::take(current_quantum_outputs);
                *render_quantum_start = Some(quantum_start);
            }
        let previous_quantum_outputs = node_runtime.first().and_then(|runtime| match runtime {
            NodeRuntime::Destination {
                previous_quantum_outputs,
                ..
            } => previous_quantum_outputs.get(frame_offset).cloned(),
            _ => None,
        });
        let k_rate_outputs = node_runtime.first().and_then(|runtime| match runtime {
            NodeRuntime::Destination {
                k_rate_quantum_start: Some(cached_quantum_start),
                k_rate_outputs,
                ..
            } if (*cached_quantum_start - quantum_start).abs() <= f64::EPSILON => {
                Some(k_rate_outputs.clone())
            }
            _ => None,
        });
        for id in &self.order {
            let id_order = self.node_order_index(*id);
            let target_config = self.nodes[id.0].channel_config;
            let input = self
                .inbound_connections
                .get(id.0)
                .into_iter()
                .flatten()
                .fold(AudioBus::silent(1), |mut mixed, connection| {
                    let output = if self.is_delay_node(*id)
                        && self
                            .node_order_index(connection.source)
                            .is_some_and(|source_order| {
                                id_order.is_some_and(|id_order| source_order > id_order)
                            }) {
                        previous_quantum_outputs
                            .as_deref()
                            .map(|previous| {
                                self.output_bus(connection.source, connection.output, previous)
                            })
                            .unwrap_or_else(|| AudioBus::silent(1))
                    } else {
                        self.output_bus(connection.source, connection.output, outputs)
                    };
                    mixed.add_assign(&apply_channel_config_bus(output, target_config));
                    mixed
                });
            outputs[id.0] = match &self.nodes[id.0].kind {
                NodeKind::Destination { .. } => input,
                NodeKind::Constant {
                    offset,
                    start_time,
                    stop_time,
                    start_scheduled,
                    ended,
                    ..
                } => {
                    let is_start_scheduled = *start_scheduled || note.is_some();
                    let start_time = if *start_scheduled { *start_time } else { 0.0 };
                    if ended.load(Ordering::SeqCst) || !is_start_scheduled {
                        AudioBus::silent(1)
                    } else if source_has_ended(time, *stop_time) {
                        if commit_source_state {
                            ended.store(true, Ordering::SeqCst);
                        }
                        AudioBus::silent(1)
                    } else if !source_is_active(time, start_time, *stop_time) {
                        AudioBus::silent(1)
                    } else {
                        let offset = self.param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::Offset,
                            },
                            offset,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        );
                        AudioBus::mono(offset)
                    }
                }
                NodeKind::Oscillator {
                    waveform,
                    frequency,
                    detune,
                    periodic_wave,
                    start_time,
                    stop_time,
                    start_scheduled,
                    ended,
                    ..
                } => {
                    let is_start_scheduled = *start_scheduled || note.is_some();
                    let start_time = if *start_scheduled { *start_time } else { 0.0 };
                    if ended.load(Ordering::SeqCst) || !is_start_scheduled {
                        AudioBus::silent(1)
                    } else if source_has_ended(time, *stop_time) {
                        if commit_source_state {
                            ended.store(true, Ordering::SeqCst);
                        }
                        AudioBus::silent(1)
                    } else if !source_is_active(time, start_time, *stop_time) {
                        AudioBus::silent(1)
                    } else {
                        let frequency = self.param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::Frequency,
                            },
                            frequency,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        );
                        let mut frequency = if let Some(note) = note
                            && !self.source_drives_audio_param(*id)
                        {
                            frequency * note.pitch_ratio()
                        } else {
                            frequency
                        };
                        let detune = self.param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::Detune,
                            },
                            detune,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        );
                        frequency *= 2.0f32.powf(detune / 1200.0);
                        let NodeRuntime::Oscillator { phase } = &mut node_runtime[id.0] else {
                            return AudioBus::silent(1);
                        };
                        let phase_for_sample = *phase as f32;
                        let sample = periodic_wave.as_ref().map_or_else(
                            || oscillator_sample_phase(*waveform, phase_for_sample),
                            |wave| wave.sample_phase(phase_for_sample),
                        );
                        if frequency.is_finite() {
                            *phase =
                                (*phase + frequency.max(0.0) as f64 * sample_dt).rem_euclid(1.0);
                        }
                        AudioBus::mono(sample)
                    }
                }
                NodeKind::Gain { gain } => {
                    let gain = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::Gain,
                        },
                        gain,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    input.scaled(gain)
                }
                NodeKind::AudioBufferSource {
                    buffer,
                    acquired_buffer,
                    playback_rate,
                    detune,
                    looping,
                    loop_range,
                    start_time,
                    stop_time,
                    start_scheduled,
                    offset,
                    duration,
                    ended,
                    ..
                } => {
                    let is_start_scheduled = *start_scheduled || note.is_some();
                    let start_time = if *start_scheduled { *start_time } else { 0.0 };
                    let render_buffer = if *start_scheduled {
                        acquired_buffer.as_ref()
                    } else {
                        buffer.as_ref()
                    };
                    let Some(buffer) = render_buffer else {
                        return AudioBus::silent(1);
                    };
                    if (note.is_none() && ended.load(Ordering::SeqCst)) || !is_start_scheduled {
                        AudioBus::silent(1)
                    } else if source_has_ended(time, *stop_time) {
                        if note.is_none() && commit_source_state {
                            ended.store(true, Ordering::SeqCst);
                        }
                        AudioBus::silent(1)
                    } else if !source_is_active(time, start_time, *stop_time) {
                        AudioBus::silent(1)
                    } else {
                        let mut playback_rate = self.param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::PlaybackRate,
                            },
                            playback_rate,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        );
                        let detune = self.param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::Detune,
                            },
                            detune,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        );
                        playback_rate *= 2.0f32.powf(detune / 1200.0);
                        if let Some(note) = note
                            && !self.source_drives_audio_param(*id)
                        {
                            playback_rate *= note.pitch_ratio();
                        }
                        let NodeRuntime::AudioBufferSource { source_time } =
                            &mut node_runtime[id.0]
                        else {
                            return AudioBus::silent(1);
                        };
                        let source_time = source_time.get_or_insert_with(|| {
                            *offset + (time - start_time).max(0.0) * playback_rate as f64
                        });
                        if buffer_source_duration_elapsed(*source_time, *offset, *duration) {
                            if note.is_none() && commit_source_state {
                                ended.store(true, Ordering::SeqCst);
                            }
                            return AudioBus::silent(buffer.number_of_channels());
                        }
                        if !*looping
                            && buffer_source_time_out_of_bounds(*source_time, buffer.duration())
                        {
                            if note.is_none() && commit_source_state {
                                ended.store(true, Ordering::SeqCst);
                            }
                            return AudioBus::silent(buffer.number_of_channels());
                        }
                        let mut render_source_time = *source_time;
                        let mut effective_loop = None;
                        if *looping {
                            let buffer_duration = buffer.duration() as f64;
                            let (loop_start, loop_end) =
                                effective_loop_range(*loop_range, buffer_duration);
                            render_source_time = wrap_loop_source_time(
                                render_source_time,
                                loop_start,
                                loop_end,
                                playback_rate,
                            );
                            effective_loop = Some((loop_start, loop_end));
                        }
                        let output = if playback_rate.abs() >= 8.0 {
                            let next_source_time =
                                render_source_time + sample_dt * playback_rate as f64;
                            buffer.bus_between(render_source_time, next_source_time, effective_loop)
                        } else if let Some((loop_start, loop_end)) = effective_loop {
                            buffer.bus_at_looping(render_source_time, loop_start, loop_end)
                        } else {
                            buffer.bus_at(render_source_time)
                        };
                        *source_time += sample_dt * playback_rate as f64;
                        output
                    }
                }
                NodeKind::ExternalSound {
                    data,
                    start_time,
                    stop_time,
                    start_scheduled,
                    ended,
                    ..
                } => {
                    let NodeRuntime::ExternalSound { sound } = &mut node_runtime[id.0] else {
                        unreachable!("external sound runtime kind");
                    };
                    if ended.load(Ordering::SeqCst) || !*start_scheduled {
                        AudioBus::silent(1)
                    } else if source_has_ended(time, *stop_time) {
                        if commit_source_state {
                            ended.store(true, Ordering::SeqCst);
                        }
                        AudioBus::silent(1)
                    } else if !source_is_active(time, *start_time, *stop_time) {
                        AudioBus::silent(1)
                    } else {
                        if sound.is_none() {
                            *sound = data.take_sound().ok();
                        }
                        if let Some(sound) = sound {
                            if sound.finished() {
                                if commit_source_state {
                                    ended.store(true, Ordering::SeqCst);
                                }
                                AudioBus::silent(1)
                            } else {
                                let mut output = [Frame::ZERO];
                                sound.process(&mut output, sample_dt, info);
                                if sound.finished() && commit_source_state {
                                    ended.store(true, Ordering::SeqCst);
                                }
                                AudioBus::from_frame(output[0])
                            }
                        } else {
                            AudioBus::silent(1)
                        }
                    }
                }
                NodeKind::StereoPanner { pan } => {
                    let pan = self
                        .param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::Pan,
                            },
                            pan,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        )
                        .clamp(-1.0, 1.0);
                    stereo_panner_bus(&input, pan)
                }
                NodeKind::BiquadFilter {
                    kind,
                    frequency,
                    detune,
                    q,
                    gain,
                } => {
                    let mut frequency = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::Frequency,
                        },
                        frequency,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let detune = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::Detune,
                        },
                        detune,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let q = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::Q,
                        },
                        q,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let gain = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::FilterGain,
                        },
                        gain,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    frequency = (frequency * 2.0f32.powf(detune / 1200.0)).max(1.0);
                    let NodeRuntime::Biquad { state } = &mut node_runtime[id.0] else {
                        unreachable!("biquad runtime kind");
                    };
                    let sample_rate = 1.0 / sample_dt;
                    let coefficients =
                        BiquadCoefficients::new(*kind, frequency, q, gain, sample_rate);
                    state.process(input, coefficients)
                }
                NodeKind::IirFilter {
                    feedforward,
                    feedback,
                } => {
                    let NodeRuntime::Iir {
                        x_history,
                        y_history,
                    } = &mut node_runtime[id.0]
                    else {
                        unreachable!("iir runtime kind");
                    };
                    process_iir(input, feedforward, feedback, x_history, y_history)
                }
                NodeKind::Delay {
                    delay_time,
                    max_delay_time,
                } => {
                    let mut delay_seconds = self
                        .param_value(
                            ParamId {
                                node: *id,
                                param: ParamKind::DelayTime,
                            },
                            delay_time,
                            sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                        )
                        .max(0.0) as f64;
                    if let Some(max_delay_time) = max_delay_time {
                        delay_seconds = delay_seconds.min(*max_delay_time as f64);
                    }
                    let delay_samples = (delay_seconds / sample_dt).max(0.0);
                    let delay_samples = if self.delay_is_in_cycle(*id) {
                        delay_samples.max(RENDER_QUANTUM_SIZE)
                    } else {
                        delay_samples
                    };
                    let buffer_len = delay_samples.ceil() as usize + 2;
                    let NodeRuntime::Delay {
                        buffer,
                        write_index,
                        delay_samples: runtime_delay_samples,
                    } = &mut node_runtime[id.0]
                    else {
                        unreachable!("delay runtime kind");
                    };
                    if buffer.len() != buffer_len
                        || buffer
                            .first()
                            .is_some_and(|sample| sample.channels.len() != input.channels.len())
                    {
                        *buffer = vec![AudioBus::silent(input.channels.len()); buffer_len];
                        *write_index = 0;
                    }
                    *runtime_delay_samples = delay_samples;
                    let delayed = process_delay_sample(buffer, *write_index, input, delay_samples);
                    *write_index = (*write_index + 1) % buffer.len();
                    delayed
                }
                NodeKind::WaveShaper { curve, oversample } => {
                    let NodeRuntime::WaveShaper { previous_input } = &mut node_runtime[id.0] else {
                        unreachable!("waveshaper runtime kind");
                    };
                    if let Some(curve) = curve {
                        process_waveshaper(input.clone(), curve, *oversample, previous_input)
                    } else {
                        *previous_input = Some(input.clone());
                        input.clone()
                    }
                }
                NodeKind::DynamicsCompressor {
                    threshold,
                    knee,
                    ratio,
                    attack,
                    release,
                    reduction,
                } => {
                    let NodeRuntime::DynamicsCompressor {
                        gain_reduction_db,
                        pre_delay,
                    } = &mut node_runtime[id.0]
                    else {
                        unreachable!("dynamics compressor runtime kind");
                    };
                    let (output, reduction_db) = compress_bus(
                        input,
                        DynamicsCompressorParams {
                            threshold_db: self.param_value(
                                ParamId {
                                    node: *id,
                                    param: ParamKind::Threshold,
                                },
                                threshold,
                                sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                            ),
                            knee_db: self.param_value(
                                ParamId {
                                    node: *id,
                                    param: ParamKind::Knee,
                                },
                                knee,
                                sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                            ),
                            ratio: self.param_value(
                                ParamId {
                                    node: *id,
                                    param: ParamKind::Ratio,
                                },
                                ratio,
                                sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                            ),
                            attack: self.param_value(
                                ParamId {
                                    node: *id,
                                    param: ParamKind::Attack,
                                },
                                attack,
                                sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                            ),
                            release: self.param_value(
                                ParamId {
                                    node: *id,
                                    param: ParamKind::Release,
                                },
                                release,
                                sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                            ),
                            sample_dt,
                        },
                        gain_reduction_db,
                        pre_delay,
                    );
                    reduction.store(reduction_db.to_bits(), Ordering::SeqCst);
                    output
                }
                NodeKind::Convolver {
                    buffer,
                    buffer_normalize,
                    ..
                } => {
                    if let Some(buffer) = buffer {
                        let NodeRuntime::Convolver { history } = &mut node_runtime[id.0] else {
                            unreachable!("convolver runtime kind");
                        };
                        process_convolver(input.clone(), buffer, *buffer_normalize, history)
                    } else {
                        AudioBus::silent(1)
                    }
                }
                NodeKind::Analyser { state } => {
                    if let Ok(mut state) = state.try_lock() {
                        state.push_bus(&input);
                    }
                    input
                }
                NodeKind::Panner {
                    position_x,
                    position_y,
                    position_z,
                    orientation_x,
                    orientation_y,
                    orientation_z,
                    distance_model,
                    ref_distance,
                    max_distance,
                    rolloff_factor,
                    cone_inner_angle,
                    cone_outer_angle,
                    cone_outer_gain,
                    ..
                } => {
                    let x = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::PositionX,
                        },
                        position_x,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let y = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::PositionY,
                        },
                        position_y,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let z = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::PositionZ,
                        },
                        position_z,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let orientation_x = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::OrientationX,
                        },
                        orientation_x,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let orientation_y = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::OrientationY,
                        },
                        orientation_y,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let orientation_z = self.param_value(
                        ParamId {
                            node: *id,
                            param: ParamKind::OrientationZ,
                        },
                        orientation_z,
                        sample_context,
                            outputs,
                            k_rate_outputs.as_deref(),
                    );
                    let listener_position = self.listener.position_at(time);
                    let listener_forward = self.listener.forward_at(time);
                    let listener_up = self.listener.up_at(time);
                    let [x, y, z] = listener_relative_coordinates(
                        [
                            x - listener_position[0],
                            y - listener_position[1],
                            z - listener_position[2],
                        ],
                        listener_forward,
                        listener_up,
                    );
                    let [orientation_x, orientation_y, orientation_z] =
                        listener_relative_coordinates(
                            [orientation_x, orientation_y, orientation_z],
                            listener_forward,
                            listener_up,
                        );
                    pan_position(
                        &input,
                        PannerSpatialParams {
                            position: [x, y, z],
                            orientation: [orientation_x, orientation_y, orientation_z],
                            distance_model: *distance_model,
                            ref_distance: *ref_distance,
                            max_distance: *max_distance,
                            rolloff_factor: *rolloff_factor,
                            cone_inner_angle: *cone_inner_angle,
                            cone_outer_angle: *cone_outer_angle,
                            cone_outer_gain: *cone_outer_gain,
                        },
                    )
                }
                NodeKind::ChannelSplitter { outputs } => {
                    if *outputs > 0 {
                        input
                    } else {
                        AudioBus::silent(1)
                    }
                }
                NodeKind::ChannelMerger { inputs } => {
                    let mut output = AudioBus::silent(*inputs);
                    for connection in self
                        .connections
                        .iter()
                        .filter(|connection| connection.destination == *id)
                    {
                        if connection.input >= *inputs {
                            continue;
                        }
                        let source_output =
                            self.output_bus(connection.source, connection.output, outputs);
                        let sample = downmix_bus_to_mono(source_output);
                        if let Some(channel) = output.channels.get_mut(connection.input) {
                            *channel += sample;
                        }
                    }
                    output
                }
                NodeKind::AudioWorklet {
                    inputs,
                    output_channel_count,
                    parameters,
                    processor_options,
                    processor,
                    ..
                } => {
                    let frame_offset = if sample_dt.is_finite() && sample_dt > 0.0 {
                        ((time - quantum_start) / sample_dt).round().max(0.0) as usize
                    } else {
                        0
                    };
                    let parameter_values = parameters
                        .iter()
                        .enumerate()
                        .map(|(index, (name, param))| {
                            let param_id = ParamId {
                                node: *id,
                                param: ParamKind::WorkletParam(index),
                            };
                            let values = match param.automation_rate() {
                                AutomationRate::ARate => (0..RENDER_QUANTUM_SIZE_USIZE)
                                    .map(|frame| {
                                        self.lookahead_param_value(
                                            param_id,
                                            param,
                                            &mut LookaheadFrame {
                                                time: quantum_start + frame as f64 * sample_dt,
                                                sample_dt,
                                                frame_offset: frame,
                                                outputs,
                                                node_runtime,
                                                stack: &mut Vec::new(),
                                            },
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                                AutomationRate::KRate => vec![self.param_value(
                                    param_id,
                                    param,
                                    RenderSample {
                                        time: quantum_start,
                                        global_time,
                                        sample_dt,
                                        commit_source_state,
                                    },
                                    outputs,
                                    k_rate_outputs.as_deref(),
                                )],
                            };
                            (name.clone(), values)
                        })
                        .collect::<HashMap<_, _>>();
                    let parameters = parameter_values
                        .iter()
                        .map(|(name, values)| {
                            (
                                name.clone(),
                                values
                                    .get(frame_offset)
                                    .or_else(|| values.first())
                                    .copied()
                                    .unwrap_or(0.0),
                            )
                        })
                        .collect();
                    let needs_quantum = match &node_runtime[id.0] {
                        NodeRuntime::AudioWorklet {
                            quantum_start: cached_quantum_start,
                            quantum_outputs,
                        } => {
                            cached_quantum_start
                                .is_none_or(|start| (start - quantum_start).abs() > f64::EPSILON)
                                || quantum_outputs.len() != RENDER_QUANTUM_SIZE_USIZE
                        }
                        _ => unreachable!("audio worklet runtime kind"),
                    };
                    let input_quantum = if needs_quantum {
                        Some(self.audio_worklet_input_quantum(
                            *id,
                            *inputs,
                            quantum_start,
                            sample_dt,
                            outputs,
                            node_runtime,
                        ))
                    } else {
                        None
                    };
                    let effective_output_channel_count =
                        output_channel_count.clone().unwrap_or_else(|| {
                            vec![
                                input_quantum
                                    .as_ref()
                                    .and_then(|inputs| inputs.first())
                                    .map(|port| port.len().clamp(1, 32))
                                    .unwrap_or(1),
                            ]
                        });
                    let rendered_output_channels =
                        effective_output_channel_count.iter().sum::<usize>().max(1);
                    let NodeRuntime::AudioWorklet {
                        quantum_start: cached_quantum_start,
                        quantum_outputs,
                    } = &mut node_runtime[id.0]
                    else {
                        unreachable!("audio worklet runtime kind");
                    };
                    if let Some(input_quantum) = input_quantum {
                        *quantum_outputs = processor.process_quantum(AudioWorkletRenderQuantum {
                            inputs: input_quantum,
                            output_channel_count: effective_output_channel_count.clone(),
                            time: quantum_start,
                            sample_dt,
                            parameters,
                            parameter_values,
                            processor_options: processor_options.clone(),
                        });
                        *cached_quantum_start = Some(quantum_start);
                    }
                    quantum_outputs
                        .get(frame_offset)
                        .cloned()
                        .unwrap_or_else(|| AudioBus::silent(rendered_output_channels))
                }
            };
        }
        if (time - quantum_start).abs() <= f64::EPSILON {
            if let Some(NodeRuntime::Destination {
                k_rate_quantum_start,
                k_rate_outputs,
                current_quantum_outputs,
                ..
            }) = node_runtime.first_mut()
            {
                *k_rate_quantum_start = Some(quantum_start);
                *k_rate_outputs = outputs.to_vec();
                if current_quantum_outputs.len() <= frame_offset {
                    current_quantum_outputs.resize(frame_offset + 1, Vec::new());
                }
                current_quantum_outputs[frame_offset] = outputs.to_vec();
            }
        } else if let Some(NodeRuntime::Destination {
            current_quantum_outputs,
            ..
        }) = node_runtime.first_mut()
        {
            if current_quantum_outputs.len() <= frame_offset {
                current_quantum_outputs.resize(frame_offset + 1, Vec::new());
            }
            current_quantum_outputs[frame_offset] = outputs.to_vec();
        }
        outputs[0].clone()
    }

    fn node_order_index(&self, node: NodeId) -> Option<usize> {
        self.order.iter().position(|candidate| *candidate == node)
    }

    fn is_delay_node(&self, id: NodeId) -> bool {
        self.nodes
            .get(id.0)
            .is_some_and(|node| matches!(node.kind, NodeKind::Delay { .. }))
    }

    fn source_drives_audio_param(&self, id: NodeId) -> bool {
        self.param_connections
            .iter()
            .any(|connection| connection.source == id)
    }

    fn delay_is_in_cycle(&self, id: NodeId) -> bool {
        self.delay_cycle_nodes.get(id.0).copied().unwrap_or(false)
    }

    fn finite_sources_finished_at(&self, time: f64, runtime: Option<&[NodeRuntime]>) -> bool {
        let mut saw_finite_source = false;
        for (index, node) in self.nodes.iter().enumerate() {
            match &node.kind {
                NodeKind::Oscillator {
                    stop_time,
                    start_scheduled,
                    ..
                }
                | NodeKind::Constant {
                    stop_time,
                    start_scheduled,
                    ..
                } => {
                    if *start_scheduled && stop_time.is_some() {
                        saw_finite_source = true;
                        if !source_has_ended(time, *stop_time) {
                            return false;
                        }
                    }
                }
                NodeKind::AudioBufferSource {
                    acquired_buffer,
                    looping,
                    start_time,
                    stop_time,
                    start_scheduled,
                    offset,
                    duration,
                    ..
                } => {
                    if *start_scheduled
                        && !looping
                        && (stop_time.is_some() || duration.is_some() || acquired_buffer.is_some())
                    {
                        saw_finite_source = true;
                        let runtime_source_time = runtime.and_then(|runtime| {
                            runtime.get(index).and_then(|runtime| match runtime {
                                NodeRuntime::AudioBufferSource { source_time } => *source_time,
                                _ => None,
                            })
                        });
                        let runtime_buffer_has_ended =
                            acquired_buffer.as_ref().is_some_and(|buffer| {
                                runtime_source_time.is_some_and(|source_time| {
                                    buffer_source_time_out_of_bounds(source_time, buffer.duration())
                                        || buffer_source_duration_elapsed(
                                            source_time,
                                            *offset,
                                            *duration,
                                        )
                                })
                            });
                        let buffer_has_ended = runtime_source_time.is_none()
                            && acquired_buffer.as_ref().is_some_and(|buffer| {
                                buffer_source_has_naturally_ended(
                                    time,
                                    *start_time,
                                    *offset,
                                    *duration,
                                    *looping,
                                    buffer,
                                )
                            });
                        if !source_has_ended(time, *stop_time)
                            && !buffer_has_ended
                            && !runtime_buffer_has_ended
                        {
                            return false;
                        }
                    }
                }
                NodeKind::ExternalSound {
                    stop_time,
                    start_scheduled,
                    ended,
                    ..
                } if *start_scheduled => {
                    let runtime_finished = runtime.is_some_and(|runtime| {
                        runtime.get(index).is_some_and(|runtime| match runtime {
                            NodeRuntime::ExternalSound { sound } => {
                                sound.as_ref().is_some_and(|sound| sound.finished())
                            }
                            _ => false,
                        })
                    });
                    let has_finite_end =
                        stop_time.is_some() || ended.load(Ordering::SeqCst) || runtime_finished;
                    if has_finite_end {
                        saw_finite_source = true;
                        if !source_has_ended(time, *stop_time)
                            && !ended.load(Ordering::SeqCst)
                            && !runtime_finished
                        {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        if saw_finite_source
            && runtime.is_some_and(|runtime| {
                self.has_pending_convolver_tail(runtime)
                    || self.has_pending_delay_tail(runtime)
                    || self.has_pending_dynamics_compressor_tail(runtime)
            })
        {
            return false;
        }
        saw_finite_source
    }

    fn has_pending_convolver_tail(&self, runtime: &[NodeRuntime]) -> bool {
        runtime.iter().any(|runtime| {
            let NodeRuntime::Convolver { history } = runtime else {
                return false;
            };
            history
                .iter()
                .take(history.len().saturating_sub(1))
                .any(audio_bus_has_signal)
        })
    }

    fn has_pending_delay_tail(&self, runtime: &[NodeRuntime]) -> bool {
        runtime.iter().any(|runtime| {
            let NodeRuntime::Delay {
                buffer,
                write_index,
                delay_samples,
            } = runtime
            else {
                return false;
            };
            let pending_age = delay_samples.ceil().max(0.0) as usize;
            if pending_age == 0 || buffer.is_empty() {
                return false;
            }
            buffer.iter().enumerate().any(|(index, bus)| {
                let age = (write_index + buffer.len() - index) % buffer.len();
                age > 0 && age <= pending_age && audio_bus_has_signal(bus)
            })
        })
    }

    fn has_pending_dynamics_compressor_tail(&self, runtime: &[NodeRuntime]) -> bool {
        runtime.iter().any(|runtime| {
            let NodeRuntime::DynamicsCompressor { pre_delay, .. } = runtime else {
                return false;
            };
            pre_delay.iter().any(|(bus, _)| audio_bus_has_signal(bus))
        })
    }

    fn output_bus(&self, source: NodeId, output: usize, outputs: &[AudioBus]) -> AudioBus {
        match &self.nodes[source.0].kind {
            NodeKind::ChannelSplitter { outputs: count } if output < *count => AudioBus::mono(
                outputs[source.0]
                    .channels
                    .get(output)
                    .copied()
                    .unwrap_or(0.0),
            ),
            NodeKind::AudioWorklet {
                output_channel_count: Some(output_channel_count),
                ..
            } if output < output_channel_count.len() => {
                let start = output_channel_count[..output].iter().sum::<usize>();
                let count = output_channel_count[output].max(1);
                let channels = (start..start + count)
                    .map(|index| {
                        outputs[source.0]
                            .channels
                            .get(index)
                            .copied()
                            .unwrap_or(0.0)
                    })
                    .collect();
                AudioBus::from_channels(channels)
            }
            NodeKind::AudioWorklet {
                outputs: output_count,
                output_channel_count: None,
                ..
            } if output < *output_count => outputs[source.0].clone(),
            _ => outputs[source.0].clone(),
        }
    }

    fn audio_worklet_input_quantum(
        &self,
        destination: NodeId,
        input_count: usize,
        quantum_start: f64,
        sample_dt: f64,
        outputs: &[AudioBus],
        node_runtime: &[NodeRuntime],
    ) -> Vec<Vec<Vec<f32>>> {
        if input_count == 0 {
            return Vec::new();
        }
        let mut input_ports = vec![Vec::<Vec<f32>>::new(); input_count];
        for frame in 0..RENDER_QUANTUM_SIZE_USIZE {
            let time = quantum_start + frame as f64 * sample_dt;
            for connection in self
                .connections
                .iter()
                .filter(|connection| connection.destination == destination)
            {
                if connection.input >= input_count {
                    continue;
                }
                if !self.audio_worklet_connection_source_active(
                    connection.source,
                    time,
                    &mut Vec::new(),
                ) {
                    continue;
                }
                let source_output = if frame == 0 {
                    self.output_bus(connection.source, connection.output, outputs)
                } else {
                    self.lookahead_node_output(
                        connection.source,
                        connection.output,
                        &mut LookaheadFrame {
                            time,
                            sample_dt,
                            frame_offset: frame,
                            outputs,
                            node_runtime,
                            stack: &mut Vec::new(),
                        },
                    )
                };
                let source_output = apply_channel_config_bus(
                    source_output,
                    self.nodes[destination.0].channel_config,
                );
                let port = &mut input_ports[connection.input];
                if port.len() < source_output.channels.len() {
                    port.resize_with(source_output.channels.len(), || {
                        vec![0.0; RENDER_QUANTUM_SIZE_USIZE]
                    });
                }
                for (channel, sample) in source_output.channels.iter().copied().enumerate() {
                    port[channel][frame] += sample;
                }
            }
        }
        input_ports
    }

    fn audio_worklet_connection_source_active(
        &self,
        source: NodeId,
        time: f64,
        stack: &mut Vec<NodeId>,
    ) -> bool {
        if stack.contains(&source) {
            return true;
        }
        stack.push(source);
        match &self.nodes[source.0].kind {
            NodeKind::Oscillator {
                start_time,
                stop_time,
                start_scheduled,
                ended,
                ..
            }
            | NodeKind::Constant {
                start_time,
                stop_time,
                start_scheduled,
                ended,
                ..
            }
            | NodeKind::ExternalSound {
                start_time,
                stop_time,
                start_scheduled,
                ended,
                ..
            } => {
                stack.pop();
                *start_scheduled
                    && !ended.load(Ordering::SeqCst)
                    && source_is_active(time, *start_time, *stop_time)
            }
            NodeKind::AudioBufferSource {
                acquired_buffer,
                start_time,
                stop_time,
                start_scheduled,
                offset,
                duration,
                playback_rate,
                detune,
                looping,
                ended,
                ..
            } => {
                if !*start_scheduled
                    || ended.load(Ordering::SeqCst)
                    || !source_is_active(time, *start_time, *stop_time)
                {
                    stack.pop();
                    return false;
                }
                if let Some(buffer) = acquired_buffer
                    && audio_buffer_source_timeline_has_ended(AudioBufferTimeline {
                        start_time: *start_time,
                        offset: *offset,
                        duration: *duration,
                        looping: *looping,
                        buffer,
                        playback_rate,
                        detune,
                        time,
                    })
                {
                    stack.pop();
                    return false;
                }
                stack.pop();
                true
            }
            NodeKind::Delay { delay_time, .. } => {
                let delayed_time = time - delay_time.clamp_value(delay_time.value_at(time)) as f64;
                let active = self.connections.iter().any(|connection| {
                    connection.destination == source
                        && (self.audio_worklet_connection_source_active(
                            connection.source,
                            time,
                            stack,
                        ) || self.audio_worklet_connection_source_active(
                            connection.source,
                            delayed_time,
                            stack,
                        ))
                });
                stack.pop();
                active
            }
            NodeKind::Convolver {
                buffer: Some(buffer),
                ..
            } => {
                let active = self.connections.iter().any(|connection| {
                    if connection.destination != source {
                        return false;
                    }
                    (0..buffer.len()).any(|tap| {
                        let input_time = time - tap as f64 / buffer.sample_rate() as f64;
                        self.audio_worklet_connection_source_active(
                            connection.source,
                            input_time,
                            stack,
                        )
                    })
                });
                stack.pop();
                active
            }
            NodeKind::DynamicsCompressor { .. } => {
                let lookahead_frames = (0.006 * self.sample_rate as f64).round() as usize;
                let active = self.connections.iter().any(|connection| {
                    if connection.destination != source {
                        return false;
                    }
                    (0..=lookahead_frames).any(|frame| {
                        let input_time = time - frame as f64 / self.sample_rate as f64;
                        self.audio_worklet_connection_source_active(
                            connection.source,
                            input_time,
                            stack,
                        )
                    })
                });
                stack.pop();
                active
            }
            NodeKind::ChannelMerger { inputs } => {
                let active = self.connections.iter().any(|connection| {
                    connection.destination == source
                        && connection.input < *inputs
                        && self.audio_worklet_connection_source_active(
                            connection.source,
                            time,
                            stack,
                        )
                });
                stack.pop();
                active
            }
            NodeKind::AudioWorklet { inputs, .. } if *inputs == 0 => {
                stack.pop();
                true
            }
            _ => {
                let active = self.connections.iter().any(|connection| {
                    connection.destination == source
                        && self.audio_worklet_connection_source_active(
                            connection.source,
                            time,
                            stack,
                        )
                });
                stack.pop();
                active
            }
        }
    }

    fn lookahead_node_output(
        &self,
        source: NodeId,
        output: usize,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        if frame.frame_offset == 0 || frame.stack.contains(&source) {
            return self.output_bus(source, output, frame.outputs);
        }
        frame.stack.push(source);
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let output_bus = match &self.nodes[source.0].kind {
            NodeKind::Constant {
                offset,
                start_time,
                stop_time,
                start_scheduled,
                ..
            } => {
                if *start_scheduled && source_is_active(frame.time, *start_time, *stop_time) {
                    AudioBus::mono(offset.clamp_value(offset.value_at(frame.time)))
                } else {
                    AudioBus::silent(1)
                }
            }
            NodeKind::AudioBufferSource {
                buffer,
                acquired_buffer,
                playback_rate,
                detune,
                looping,
                loop_range,
                start_time,
                stop_time,
                start_scheduled,
                offset,
                duration,
                ..
            } => self.lookahead_audio_buffer_source_output(
                LookaheadBufferSource {
                    node: source,
                    buffer,
                    acquired_buffer,
                    playback_rate,
                    detune,
                    looping: *looping,
                    loop_range: *loop_range,
                    start_time: *start_time,
                    stop_time: *stop_time,
                    start_scheduled: *start_scheduled,
                    offset: *offset,
                    duration: *duration,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::Oscillator {
                waveform,
                frequency,
                detune,
                periodic_wave,
                start_time,
                stop_time,
                start_scheduled,
                ..
            } => self.lookahead_oscillator_output(
                LookaheadOscillator {
                    node: source,
                    waveform: *waveform,
                    periodic_wave: periodic_wave.as_ref(),
                    frequency,
                    detune,
                    start_time: *start_time,
                    stop_time: *stop_time,
                    start_scheduled: *start_scheduled,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::Gain { gain } => {
                let input = self.lookahead_node_input(
                    source,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                input.scaled(gain.clamp_value(gain.value_at(time)))
            }
            NodeKind::StereoPanner { pan } => {
                let input = self.lookahead_node_input(
                    source,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let pan = self
                    .lookahead_param_value(
                        ParamId {
                            node: source,
                            param: ParamKind::Pan,
                        },
                        pan,
                        &mut LookaheadFrame {
                            time,
                            sample_dt,
                            frame_offset,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    )
                    .clamp(-1.0, 1.0);
                stereo_panner_bus(&input, pan)
            }
            NodeKind::Panner {
                position_x,
                position_y,
                position_z,
                orientation_x,
                orientation_y,
                orientation_z,
                distance_model,
                ref_distance,
                max_distance,
                rolloff_factor,
                cone_inner_angle,
                cone_outer_angle,
                cone_outer_gain,
                ..
            } => {
                let input = self.lookahead_node_input(
                    source,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let x = self.lookahead_param_value(
                    ParamId {
                        node: source,
                        param: ParamKind::PositionX,
                    },
                    position_x,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let y = self.lookahead_param_value(
                    ParamId {
                        node: source,
                        param: ParamKind::PositionY,
                    },
                    position_y,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let z = self.lookahead_param_value(
                    ParamId {
                        node: source,
                        param: ParamKind::PositionZ,
                    },
                    position_z,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let orientation_x = self.lookahead_param_value(
                    ParamId {
                        node: source,
                        param: ParamKind::OrientationX,
                    },
                    orientation_x,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let orientation_y = self.lookahead_param_value(
                    ParamId {
                        node: source,
                        param: ParamKind::OrientationY,
                    },
                    orientation_y,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let orientation_z = self.lookahead_param_value(
                    ParamId {
                        node: source,
                        param: ParamKind::OrientationZ,
                    },
                    orientation_z,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                let listener_time = time + frame_offset as f64 * sample_dt;
                let listener_position = self.listener.position_at(listener_time);
                let listener_forward = self.listener.forward_at(listener_time);
                let listener_up = self.listener.up_at(listener_time);
                let [x, y, z] = listener_relative_coordinates(
                    [
                        x - listener_position[0],
                        y - listener_position[1],
                        z - listener_position[2],
                    ],
                    listener_forward,
                    listener_up,
                );
                let [orientation_x, orientation_y, orientation_z] = listener_relative_coordinates(
                    [orientation_x, orientation_y, orientation_z],
                    listener_forward,
                    listener_up,
                );
                pan_position(
                    &input,
                    PannerSpatialParams {
                        position: [x, y, z],
                        orientation: [orientation_x, orientation_y, orientation_z],
                        distance_model: *distance_model,
                        ref_distance: *ref_distance,
                        max_distance: *max_distance,
                        rolloff_factor: *rolloff_factor,
                        cone_inner_angle: *cone_inner_angle,
                        cone_outer_angle: *cone_outer_angle,
                        cone_outer_gain: *cone_outer_gain,
                    },
                )
            }
            NodeKind::ChannelSplitter { outputs: count } if output < *count => {
                let input = self.lookahead_node_input(
                    source,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                AudioBus::mono(input.channels.get(output).copied().unwrap_or(0.0))
            }
            NodeKind::ChannelMerger { inputs } => {
                let mut output = AudioBus::silent(*inputs);
                for connection in self
                    .connections
                    .iter()
                    .filter(|connection| connection.destination == source)
                {
                    if connection.input >= *inputs {
                        continue;
                    }
                    let source_output = self.lookahead_node_output(
                        connection.source,
                        connection.output,
                        &mut LookaheadFrame {
                            time,
                            sample_dt,
                            frame_offset,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    );
                    let sample = downmix_bus_to_mono(source_output);
                    if let Some(channel) = output.channels.get_mut(connection.input) {
                        *channel += sample;
                    }
                }
                output
            }
            NodeKind::WaveShaper { curve, oversample } => {
                let input = self.lookahead_node_input(
                    source,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                if let Some(curve) = curve {
                    let previous_time = time - sample_dt;
                    let previous_input = self.lookahead_node_input(
                        source,
                        &mut LookaheadFrame {
                            time: previous_time,
                            sample_dt,
                            frame_offset: frame_offset.saturating_sub(1),
                            outputs,
                            node_runtime,
                            stack,
                        },
                    );
                    process_waveshaper(input, curve, *oversample, &mut Some(previous_input))
                } else {
                    input
                }
            }
            NodeKind::BiquadFilter {
                kind,
                frequency,
                detune,
                q,
                gain,
            } => self.lookahead_biquad_output(
                LookaheadBiquad {
                    node: source,
                    kind: *kind,
                    frequency,
                    detune,
                    q,
                    gain,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::IirFilter {
                feedforward,
                feedback,
            } => self.lookahead_iir_output(
                LookaheadIir {
                    node: source,
                    feedforward,
                    feedback,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::Delay {
                delay_time,
                max_delay_time,
            } => self.lookahead_delay_output(
                LookaheadDelay {
                    node: source,
                    delay_time,
                    max_delay_time: *max_delay_time,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::Convolver {
                buffer,
                buffer_normalize,
                ..
            } => self.lookahead_convolver_output(
                LookaheadConvolver {
                    node: source,
                    buffer: buffer.as_ref(),
                    normalize: *buffer_normalize,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::DynamicsCompressor {
                threshold,
                knee,
                ratio,
                attack,
                release,
                ..
            } => self.lookahead_dynamics_compressor_output(
                LookaheadDynamics {
                    node: source,
                    threshold,
                    knee,
                    ratio,
                    attack,
                    release,
                },
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            NodeKind::Analyser { .. } => self.lookahead_node_input(
                source,
                &mut LookaheadFrame {
                    time,
                    sample_dt,
                    frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ),
            _ => self.output_bus(source, output, outputs),
        };
        stack.pop();
        output_bus
    }

    fn lookahead_biquad_output(
        &self,
        biquad: LookaheadBiquad<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let LookaheadBiquad {
            node,
            kind,
            frequency,
            detune,
            q,
            gain,
        } = biquad;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let Some(NodeRuntime::Biquad { state }) = node_runtime.get(node.0) else {
            return self.output_bus(node, 0, outputs);
        };
        let mut state = state.clone();
        let quantum_start = time - frame_offset as f64 * sample_dt;
        let mut output = self.output_bus(node, 0, outputs);
        for frame in 1..=frame_offset {
            let frame_time = quantum_start + frame as f64 * sample_dt;
            let input = self.lookahead_node_input(
                node,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let mut frequency_value = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::Frequency,
                },
                frequency,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let detune_value = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::Detune,
                },
                detune,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let q_value = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::Q,
                },
                q,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let gain_value = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::FilterGain,
                },
                gain,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            frequency_value = (frequency_value * 2.0f32.powf(detune_value / 1200.0)).max(1.0);
            let sample_rate = 1.0 / sample_dt;
            let coefficients =
                BiquadCoefficients::new(kind, frequency_value, q_value, gain_value, sample_rate);
            output = state.process(input, coefficients);
        }
        output
    }

    fn lookahead_iir_output(
        &self,
        iir: LookaheadIir<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let LookaheadIir {
            node,
            feedforward,
            feedback,
        } = iir;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let Some(NodeRuntime::Iir {
            x_history,
            y_history,
        }) = node_runtime.get(node.0)
        else {
            return self.output_bus(node, 0, outputs);
        };
        let mut x_history = x_history.clone();
        let mut y_history = y_history.clone();
        let quantum_start = time - frame_offset as f64 * sample_dt;
        let mut output = self.output_bus(node, 0, outputs);
        for frame in 1..=frame_offset {
            let frame_time = quantum_start + frame as f64 * sample_dt;
            let input = self.lookahead_node_input(
                node,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            output = process_iir(input, feedforward, feedback, &mut x_history, &mut y_history);
        }
        output
    }

    fn lookahead_delay_output(
        &self,
        delay: LookaheadDelay<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let LookaheadDelay {
            node,
            delay_time,
            max_delay_time,
        } = delay;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let Some(NodeRuntime::Delay {
            buffer,
            write_index,
            ..
        }) = node_runtime.get(node.0)
        else {
            return self.output_bus(node, 0, outputs);
        };
        let mut buffer = buffer.clone();
        let mut write_index = *write_index;
        let quantum_start = time - frame_offset as f64 * sample_dt;
        let mut output = self.output_bus(node, 0, outputs);
        for frame in 1..=frame_offset {
            let frame_time = quantum_start + frame as f64 * sample_dt;
            let input = self.lookahead_node_input(
                node,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let mut delay_seconds = self
                .lookahead_param_value(
                    ParamId {
                        node,
                        param: ParamKind::DelayTime,
                    },
                    delay_time,
                    &mut LookaheadFrame {
                        time: frame_time,
                        sample_dt,
                        frame_offset: frame,
                        outputs,
                        node_runtime,
                        stack,
                    },
                )
                .max(0.0) as f64;
            if let Some(max_delay_time) = max_delay_time {
                delay_seconds = delay_seconds.min(max_delay_time as f64);
            }
            let delay_samples = (delay_seconds / sample_dt).max(0.0);
            let delay_samples = if self.delay_is_in_cycle(node) {
                delay_samples.max(RENDER_QUANTUM_SIZE)
            } else {
                delay_samples
            };
            let buffer_len = delay_samples.ceil() as usize + 2;
            if buffer.len() != buffer_len
                || buffer
                    .first()
                    .is_some_and(|sample| sample.channels.len() != input.channels.len())
            {
                buffer = vec![AudioBus::silent(input.channels.len()); buffer_len];
                write_index = 0;
            }
            output = process_delay_sample(&mut buffer, write_index, input, delay_samples);
            write_index = (write_index + 1) % buffer.len();
        }
        output
    }

    fn lookahead_convolver_output(
        &self,
        convolver: LookaheadConvolver<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let LookaheadConvolver {
            node,
            buffer,
            normalize,
        } = convolver;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let Some(buffer) = buffer else {
            return AudioBus::silent(1);
        };
        let Some(NodeRuntime::Convolver { history }) = node_runtime.get(node.0) else {
            return self.output_bus(node, 0, outputs);
        };
        let mut history = history.clone();
        let quantum_start = time - frame_offset as f64 * sample_dt;
        let mut output = self.output_bus(node, 0, outputs);
        for frame in 1..=frame_offset {
            let frame_time = quantum_start + frame as f64 * sample_dt;
            let input = self.lookahead_node_input(
                node,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            output = process_convolver(input, buffer, normalize, &mut history);
        }
        output
    }

    fn lookahead_dynamics_compressor_output(
        &self,
        dynamics: LookaheadDynamics<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let LookaheadDynamics {
            node,
            threshold,
            knee,
            ratio,
            attack,
            release,
        } = dynamics;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let Some(NodeRuntime::DynamicsCompressor {
            gain_reduction_db,
            pre_delay,
        }) = node_runtime.get(node.0)
        else {
            return self.output_bus(node, 0, outputs);
        };
        let mut gain_reduction_db = *gain_reduction_db;
        let mut pre_delay = pre_delay.clone();
        let quantum_start = time - frame_offset as f64 * sample_dt;
        let mut output = self.output_bus(node, 0, outputs);
        for frame in 1..=frame_offset {
            let frame_time = quantum_start + frame as f64 * sample_dt;
            let input = self.lookahead_node_input(
                node,
                &mut LookaheadFrame {
                    time: frame_time,
                    sample_dt,
                    frame_offset: frame,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let (compressed, _) = compress_bus(
                input,
                DynamicsCompressorParams {
                    threshold_db: self.lookahead_param_value(
                        ParamId {
                            node,
                            param: ParamKind::Threshold,
                        },
                        threshold,
                        &mut LookaheadFrame {
                            time: frame_time,
                            sample_dt,
                            frame_offset: frame,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    ),
                    knee_db: self.lookahead_param_value(
                        ParamId {
                            node,
                            param: ParamKind::Knee,
                        },
                        knee,
                        &mut LookaheadFrame {
                            time: frame_time,
                            sample_dt,
                            frame_offset: frame,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    ),
                    ratio: self.lookahead_param_value(
                        ParamId {
                            node,
                            param: ParamKind::Ratio,
                        },
                        ratio,
                        &mut LookaheadFrame {
                            time: frame_time,
                            sample_dt,
                            frame_offset: frame,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    ),
                    attack: self.lookahead_param_value(
                        ParamId {
                            node,
                            param: ParamKind::Attack,
                        },
                        attack,
                        &mut LookaheadFrame {
                            time: frame_time,
                            sample_dt,
                            frame_offset: frame,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    ),
                    release: self.lookahead_param_value(
                        ParamId {
                            node,
                            param: ParamKind::Release,
                        },
                        release,
                        &mut LookaheadFrame {
                            time: frame_time,
                            sample_dt,
                            frame_offset: frame,
                            outputs,
                            node_runtime,
                            stack,
                        },
                    ),
                    sample_dt,
                },
                &mut gain_reduction_db,
                &mut pre_delay,
            );
            output = compressed;
        }
        output
    }

    fn lookahead_node_input(
        &self,
        destination: NodeId,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let target_config = self.nodes[destination.0].channel_config;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        self.connections
            .iter()
            .filter(|connection| connection.destination == destination)
            .fold(AudioBus::silent(1), |mut mixed, connection| {
                let output = self.lookahead_node_output(
                    connection.source,
                    connection.output,
                    &mut LookaheadFrame {
                        time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                );
                mixed.add_assign(&apply_channel_config_bus(output, target_config));
                mixed
            })
    }

    fn lookahead_audio_buffer_source_output(
        &self,
        source: LookaheadBufferSource<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        let render_buffer = if source.start_scheduled {
            source.acquired_buffer.as_ref()
        } else {
            source.buffer.as_ref()
        };
        let Some(buffer) = render_buffer else {
            return AudioBus::silent(1);
        };
        if !source.start_scheduled
            || !source_is_active(frame.time, source.start_time, source.stop_time)
        {
            return AudioBus::silent(buffer.number_of_channels());
        }
        let mut source_time = self.lookahead_audio_buffer_source_time(
            &source,
            frame,
        );
        if buffer_source_duration_elapsed(source_time, source.offset, source.duration) {
            return AudioBus::silent(buffer.number_of_channels());
        }
        if !source.looping && buffer_source_time_out_of_bounds(source_time, buffer.duration()) {
            return AudioBus::silent(buffer.number_of_channels());
        }
        if source.looping {
            let buffer_duration = buffer.duration() as f64;
            let (loop_start, loop_end) = effective_loop_range(source.loop_range, buffer_duration);
            let playback_rate = self.lookahead_audio_buffer_effective_playback_rate(
                &source,
                frame,
            );
            source_time = wrap_loop_source_time(source_time, loop_start, loop_end, playback_rate);
            return buffer.bus_at_looping(source_time, loop_start, loop_end);
        }
        buffer.bus_at(source_time)
    }

    fn lookahead_audio_buffer_source_time(
        &self,
        source: &LookaheadBufferSource<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> f64 {
        let node = source.node;
        let playback_rate = source.playback_rate;
        let detune = source.detune;
        let start_time = source.start_time;
        let offset = source.offset;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        if time <= start_time {
            return offset;
        }
        let quantum_start = if sample_dt.is_finite() && sample_dt > 0.0 {
            time - frame_offset as f64 * sample_dt
        } else {
            time
        };
        let step = if sample_dt.is_finite() && sample_dt > 0.0 {
            sample_dt
        } else {
            time - start_time
        };
        let mut source_time = offset;
        let mut cursor = start_time;
        while cursor < time {
            let dt = (time - cursor).min(step);
            let local_frame_offset =
                if sample_dt.is_finite() && sample_dt > 0.0 && cursor >= quantum_start {
                    ((cursor - quantum_start) / sample_dt).round().max(0.0) as usize
                } else {
                    0
                };
            let mut rate = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::PlaybackRate,
                },
                playback_rate,
                &mut LookaheadFrame {
                    time: cursor,
                    sample_dt,
                    frame_offset: local_frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            let detune = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::Detune,
                },
                detune,
                &mut LookaheadFrame {
                    time: cursor,
                    sample_dt,
                    frame_offset: local_frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            );
            rate *= 2.0f32.powf(detune / 1200.0);
            if rate.is_finite() {
                source_time += dt * rate as f64;
            }
            cursor += dt;
        }
        source_time
    }

    fn lookahead_audio_buffer_effective_playback_rate(
        &self,
        source: &LookaheadBufferSource<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> f32 {
        let node = source.node;
        let playback_rate = source.playback_rate;
        let detune = source.detune;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let rate = self.lookahead_param_value(
            ParamId {
                node,
                param: ParamKind::PlaybackRate,
            },
            playback_rate,
            &mut LookaheadFrame {
                time,
                sample_dt,
                frame_offset,
                outputs,
                node_runtime,
                stack,
            },
        );
        let detune = self.lookahead_param_value(
            ParamId {
                node,
                param: ParamKind::Detune,
            },
            detune,
            &mut LookaheadFrame {
                time,
                sample_dt,
                frame_offset,
                outputs,
                node_runtime,
                stack,
            },
        );
        rate * 2.0f32.powf(detune / 1200.0)
    }

    fn lookahead_oscillator_output(
        &self,
        oscillator: LookaheadOscillator<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> AudioBus {
        if !oscillator.start_scheduled
            || !source_is_active(frame.time, oscillator.start_time, oscillator.stop_time)
        {
            return AudioBus::silent(1);
        }
        let phase = self.lookahead_oscillator_phase(
            &oscillator,
            frame,
        );
        let sample = oscillator.periodic_wave.as_ref().map_or_else(
            || oscillator_sample_phase(oscillator.waveform, phase as f32),
            |wave| wave.sample_phase(phase as f32),
        );
        AudioBus::mono(sample)
    }

    fn lookahead_oscillator_phase(
        &self,
        oscillator: &LookaheadOscillator<'_>,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> f64 {
        let node = oscillator.node;
        let frequency = oscillator.frequency;
        let detune = oscillator.detune;
        let start_time = oscillator.start_time;
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        if time <= start_time {
            return 0.0;
        }
        let step = if sample_dt.is_finite() && sample_dt > 0.0 {
            sample_dt
        } else {
            time - start_time
        };
        let mut phase = 0.0;
        let mut cursor = start_time;
        while cursor < time {
            let dt = (time - cursor).min(step);
            let local_frame_offset = if sample_dt.is_finite() && sample_dt > 0.0 {
                ((cursor - (time - frame_offset as f64 * sample_dt)) / sample_dt)
                    .round()
                    .max(0.0) as usize
            } else {
                frame_offset
            };
            let mut hz = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::Frequency,
                },
                frequency,
                &mut LookaheadFrame {
                    time: cursor,
                    sample_dt,
                    frame_offset: local_frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ) as f64;
            let detune = self.lookahead_param_value(
                ParamId {
                    node,
                    param: ParamKind::Detune,
                },
                detune,
                &mut LookaheadFrame {
                    time: cursor,
                    sample_dt,
                    frame_offset: local_frame_offset,
                    outputs,
                    node_runtime,
                    stack,
                },
            ) as f64;
            hz *= 2.0f64.powf(detune / 1200.0);
            if hz.is_finite() {
                phase += hz.max(0.0) * dt;
            }
            cursor += dt;
        }
        phase.rem_euclid(1.0)
    }

    fn lookahead_param_value(
        &self,
        param_id: ParamId,
        param: &ParamTimeline,
        frame: &mut LookaheadFrame<'_, '_>,
    ) -> f32 {
        let time = frame.time;
        let sample_dt = frame.sample_dt;
        let frame_offset = frame.frame_offset;
        let outputs = frame.outputs;
        let node_runtime = frame.node_runtime;
        let stack = &mut *frame.stack;
        let automation_time = match param.automation_rate() {
            AutomationRate::ARate => time,
            AutomationRate::KRate if sample_dt.is_finite() && sample_dt > 0.0 => {
                k_rate_quantum_start(time, sample_dt)
            }
            AutomationRate::KRate => time,
        };
        let modulation = self
            .param_connections
            .iter()
            .filter(|connection| connection.destination == param_id)
            .map(|connection| {
                downmix_bus_to_mono(self.lookahead_node_output(
                    connection.source,
                    connection.output,
                    &mut LookaheadFrame {
                        time: automation_time,
                        sample_dt,
                        frame_offset,
                        outputs,
                        node_runtime,
                        stack,
                    },
                ))
            })
            .sum::<f32>();
        param.clamp_value(param.value_at(automation_time) + modulation)
    }

    fn param_value(
        &self,
        param_id: ParamId,
        param: &ParamTimeline,
        sample: RenderSample,
        outputs: &[AudioBus],
        k_rate_outputs: Option<&[AudioBus]>,
    ) -> f32 {
        let RenderSample {
            time,
            global_time,
            sample_dt,
            ..
        } = sample;
        let timeline_time = match param.time_domain {
            ParamTimeDomain::Local => time,
            ParamTimeDomain::Global => global_time,
        };
        let automation_time = match param.automation_rate() {
            AutomationRate::ARate => timeline_time,
            AutomationRate::KRate if sample_dt.is_finite() && sample_dt > 0.0 => {
                let quantum_duration = sample_dt * RENDER_QUANTUM_SIZE;
                (timeline_time / quantum_duration).floor() * quantum_duration
            }
            AutomationRate::KRate => timeline_time,
        };
        let modulation = self
            .param_connections
            .iter()
            .filter(|connection| connection.destination == param_id)
            .map(|connection| {
                self.param_connection_value(
                    connection,
                    param.automation_rate(),
                    time,
                    sample_dt,
                    outputs,
                    k_rate_outputs,
                )
            })
            .sum::<f32>();
        param.clamp_value(param.value_at(automation_time) + modulation)
    }

    fn param_connection_value(
        &self,
        connection: &ParamConnection,
        automation_rate: AutomationRate,
        time: f64,
        sample_dt: f64,
        outputs: &[AudioBus],
        k_rate_outputs: Option<&[AudioBus]>,
    ) -> f32 {
        let sample_time = match automation_rate {
            AutomationRate::ARate => time,
            AutomationRate::KRate if sample_dt.is_finite() && sample_dt > 0.0 => {
                let quantum_duration = sample_dt * RENDER_QUANTUM_SIZE;
                (time / quantum_duration).floor() * quantum_duration
            }
            AutomationRate::KRate => time,
        };
        if (sample_time - time).abs() <= f64::EPSILON {
            return downmix_bus_to_mono(self.output_bus(
                connection.source,
                connection.output,
                outputs,
            ));
        }
        if let Some(k_rate_outputs) = k_rate_outputs {
            return downmix_bus_to_mono(self.output_bus(
                connection.source,
                connection.output,
                k_rate_outputs,
            ));
        }
        match &self.nodes[connection.source.0].kind {
            NodeKind::Constant {
                offset,
                start_time,
                stop_time,
                start_scheduled,
                ..
            } if *start_scheduled && source_is_active(sample_time, *start_time, *stop_time) => {
                offset.clamp_value(offset.value_at(sample_time))
            }
            _ => {
                downmix_bus_to_mono(self.output_bus(connection.source, connection.output, outputs))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioContextSoundData {
    graph: AudioContext,
    sample_rate: u32,
}

impl AudioContextSoundData {
    #[must_use]
    pub fn sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate.max(1);
        self
    }
}

impl SoundData for AudioContextSoundData {
    type Error = GraphError;
    type Handle = AudioContextSoundHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let state = Arc::new(AudioContextSoundHandleState::default());
        let handle = AudioContextSoundHandle {
            state: Arc::clone(&state),
        };
        Ok((
            Box::new(GraphSound::new(
                self.graph.compiled()?,
                self.sample_rate,
                state,
            )?),
            handle,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct AudioContextSoundHandle {
    state: Arc<AudioContextSoundHandleState>,
}

impl AudioContextSoundHandle {
    pub fn stop(&self) {
        self.state.stopped.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn stopped(&self) -> bool {
        self.state.stopped.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Default)]
struct AudioContextSoundHandleState {
    stopped: AtomicBool,
}

#[derive(Debug)]
struct GraphSound {
    graph: CompiledGraph,
    runtime: Vec<NodeRuntime>,
    sample_rate: u32,
    elapsed: f64,
    state: Arc<AudioContextSoundHandleState>,
}

impl GraphSound {
    fn new(
        graph: CompiledGraph,
        sample_rate: u32,
        state: Arc<AudioContextSoundHandleState>,
    ) -> Result<Self, GraphError> {
        let runtime = graph.runtime()?;
        Ok(Self {
            graph,
            runtime,
            sample_rate,
            elapsed: 0.0,
            state,
        })
    }
}

impl Sound for GraphSound {
    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        if self.state.stopped.load(Ordering::Relaxed) {
            out.fill(Frame::ZERO);
            return;
        }
        let sample_dt = if dt.is_finite() && dt > 0.0 {
            dt
        } else {
            1.0 / self.sample_rate as f64
        };
        let mut offset = 0;
        while offset < out.len() {
            let quantum_frames = (out.len() - offset).min(RENDER_QUANTUM_SIZE_USIZE);
            let quantum = self.graph.render_quantum_with_runtime(
                RenderQuantum {
                    start: self.elapsed,
                    global_start: self.elapsed,
                    sample_dt,
                    frames: quantum_frames,
                    commit_source_state: true,
                },
                None,
                &mut self.runtime,
                info,
            );
            out[offset..offset + quantum_frames].copy_from_slice(&quantum);
            offset += quantum_frames;
            self.elapsed += sample_dt * quantum_frames as f64;
        }
    }

    fn finished(&self) -> bool {
        if self.state.stopped.load(Ordering::Relaxed) {
            return true;
        }
        self.graph
            .finite_sources_finished_at(self.elapsed, Some(&self.runtime))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineAudioContextState {
    Suspended,
    Running,
    Closed,
}

#[derive(Debug, Clone)]
pub struct OfflineAudioContext {
    number_of_channels: usize,
    length: usize,
    sample_rate: u32,
    current_time: f64,
    state: OfflineAudioContextState,
    suspend_frames: Vec<usize>,
    graph: AudioContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineAudioContextOptions {
    pub number_of_channels: usize,
    pub length: usize,
    pub sample_rate: u32,
    pub render_size_hint: Option<usize>,
}

impl OfflineAudioContext {
    #[must_use]
    fn new(number_of_channels: usize, length: usize, sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        Self {
            number_of_channels: number_of_channels.max(1),
            length,
            sample_rate,
            current_time: 0.0,
            state: OfflineAudioContextState::Suspended,
            suspend_frames: Vec::new(),
            graph: AudioContext::with_sample_rate_and_destination_channels(
                sample_rate,
                number_of_channels.max(1),
                None,
            ),
        }
    }

    pub fn try_new(
        number_of_channels: usize,
        length: usize,
        sample_rate: u32,
    ) -> Result<Self, GraphError> {
        if number_of_channels == 0
            || number_of_channels > 32
            || length == 0
            || !(3_000..=384_000).contains(&sample_rate)
        {
            return Err(GraphError::InvalidAudioBuffer);
        }
        Ok(Self::new(number_of_channels, length, sample_rate))
    }

    pub fn try_new_with_options(options: OfflineAudioContextOptions) -> Result<Self, GraphError> {
        if options.render_size_hint.is_some_and(|hint| hint == 0) {
            return Err(GraphError::InvalidAudioBuffer);
        }
        Self::try_new(
            options.number_of_channels,
            options.length,
            options.sample_rate,
        )
    }

    #[must_use]
    pub fn number_of_channels(&self) -> usize {
        self.number_of_channels
    }

    #[must_use]
    pub fn length(&self) -> usize {
        self.length
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub fn render_quantum_size(&self) -> usize {
        RENDER_QUANTUM_SIZE_USIZE
    }

    #[must_use]
    pub fn current_time(&self) -> f64 {
        self.current_time
    }

    #[must_use]
    pub fn state(&self) -> OfflineAudioContextState {
        self.state
    }

    pub fn suspend(&mut self, time: f64) -> Result<(), GraphError> {
        if self.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        if time < 0.0 {
            return Err(GraphError::NegativeTime);
        }
        if !time.is_finite() {
            return Err(GraphError::InvalidAutomationValue);
        }
        let frame = (time * self.sample_rate as f64).floor() as usize;
        if frame >= self.length || self.suspend_frames.contains(&frame) {
            return Err(GraphError::InvalidState);
        }
        self.suspend_frames.push(frame);
        self.current_time = frame as f64 / self.sample_rate as f64;
        self.state = OfflineAudioContextState::Suspended;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), GraphError> {
        if self.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        Err(GraphError::InvalidState)
    }

    pub fn close(&mut self) -> Result<(), GraphError> {
        if self.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        self.state = OfflineAudioContextState::Closed;
        Ok(())
    }

    pub fn try_create_buffer(
        &self,
        number_of_channels: usize,
        length: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, GraphError> {
        self.graph
            .try_create_buffer(number_of_channels, length, sample_rate)
    }

    pub fn create_buffer(
        &self,
        number_of_channels: usize,
        length: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, GraphError> {
        self.graph
            .create_buffer(number_of_channels, length, sample_rate)
    }

    pub fn try_create_buffer_with_options(
        &self,
        options: AudioBufferOptions,
    ) -> Result<AudioBuffer, GraphError> {
        self.graph.try_create_buffer_with_options(options)
    }

    pub fn try_create_periodic_wave(
        &self,
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
    ) -> Result<PeriodicWave, GraphError> {
        self.graph.try_create_periodic_wave(real, imag)
    }

    pub fn create_periodic_wave(
        &self,
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
    ) -> Result<PeriodicWave, GraphError> {
        self.graph.create_periodic_wave(real, imag)
    }

    pub fn try_create_periodic_wave_with_options(
        &self,
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
        options: PeriodicWaveOptions,
    ) -> Result<PeriodicWave, GraphError> {
        self.graph
            .try_create_periodic_wave_with_options(real, imag, options)
    }

    #[must_use]
    pub fn destination(&self) -> AudioDestinationNode {
        self.graph.destination()
    }

    #[must_use]
    pub fn listener(&self) -> AudioListener {
        self.graph.listener()
    }

    #[must_use]
    pub fn create_oscillator(&mut self) -> OscillatorNode {
        self.graph.create_oscillator()
    }

    pub fn try_create_oscillator_with_options(
        &mut self,
        options: OscillatorOptions,
    ) -> Result<OscillatorNode, GraphError> {
        self.graph.try_create_oscillator_with_options(options)
    }

    #[must_use]
    pub fn create_constant_source(&mut self) -> ConstantSourceNode {
        self.graph.create_constant_source()
    }

    pub fn try_create_constant_source_with_options(
        &mut self,
        options: ConstantSourceOptions,
    ) -> Result<ConstantSourceNode, GraphError> {
        self.graph.try_create_constant_source_with_options(options)
    }

    #[must_use]
    pub fn create_gain(&mut self) -> GainNode {
        self.graph.create_gain()
    }

    pub fn try_create_gain_with_options(
        &mut self,
        options: GainOptions,
    ) -> Result<GainNode, GraphError> {
        self.graph.try_create_gain_with_options(options)
    }

    #[must_use]
    pub fn create_buffer_source(&mut self) -> AudioBufferSourceNode {
        self.graph.create_buffer_source()
    }

    pub fn try_create_buffer_source_with_options(
        &mut self,
        options: AudioBufferSourceOptions,
    ) -> Result<AudioBufferSourceNode, GraphError> {
        self.graph.try_create_buffer_source_with_options(options)
    }

    #[must_use]
    pub fn create_sound_data_source<D>(&mut self, data: D) -> SoundDataSourceNode
    where
        D: SoundData + Send + 'static,
        D::Error: fmt::Debug + Send + Sync + 'static,
    {
        self.graph.create_sound_data_source(data)
    }

    #[must_use]
    pub fn create_biquad_filter(&mut self) -> BiquadFilterHandle {
        self.graph.create_biquad_filter()
    }

    pub fn try_create_biquad_filter_with_options(
        &mut self,
        options: BiquadFilterOptions,
    ) -> Result<BiquadFilterHandle, GraphError> {
        self.graph.try_create_biquad_filter_with_options(options)
    }

    pub fn try_create_iir_filter(
        &mut self,
        feedforward: impl IntoIterator<Item = f32>,
        feedback: impl IntoIterator<Item = f32>,
    ) -> Result<IirFilterNode, GraphError> {
        self.graph.try_create_iir_filter(feedforward, feedback)
    }

    pub fn create_iir_filter(
        &mut self,
        feedforward: impl IntoIterator<Item = f32>,
        feedback: impl IntoIterator<Item = f32>,
    ) -> Result<IirFilterNode, GraphError> {
        self.graph.create_iir_filter(feedforward, feedback)
    }

    pub fn try_create_iir_filter_with_options(
        &mut self,
        options: IirFilterOptions,
    ) -> Result<IirFilterNode, GraphError> {
        self.graph.try_create_iir_filter_with_options(options)
    }

    #[must_use]
    pub fn create_delay(&mut self) -> DelayNodeHandle {
        self.graph.create_delay()
    }

    pub fn try_create_delay(&mut self, max_delay_time: f64) -> Result<DelayNodeHandle, GraphError> {
        self.graph.try_create_delay(max_delay_time)
    }

    pub fn try_create_delay_with_options(
        &mut self,
        options: DelayOptions,
    ) -> Result<DelayNodeHandle, GraphError> {
        self.graph.try_create_delay_with_options(options)
    }

    #[must_use]
    pub fn create_wave_shaper(&mut self) -> WaveShaperNode {
        self.graph.create_wave_shaper()
    }

    pub fn try_create_wave_shaper_with_options(
        &mut self,
        options: WaveShaperOptions,
    ) -> Result<WaveShaperNode, GraphError> {
        self.graph.try_create_wave_shaper_with_options(options)
    }

    #[must_use]
    pub fn create_convolver(&mut self) -> ConvolverNode {
        self.graph.create_convolver()
    }

    pub fn try_create_convolver_with_options(
        &mut self,
        options: ConvolverOptions,
    ) -> Result<ConvolverNode, GraphError> {
        self.graph.try_create_convolver_with_options(options)
    }

    #[must_use]
    pub fn create_stereo_panner(&mut self) -> StereoPannerNode {
        self.graph.create_stereo_panner()
    }

    pub fn try_create_stereo_panner_with_options(
        &mut self,
        options: StereoPannerOptions,
    ) -> Result<StereoPannerNode, GraphError> {
        self.graph.try_create_stereo_panner_with_options(options)
    }

    #[must_use]
    pub fn create_dynamics_compressor(&mut self) -> DynamicsCompressorNode {
        self.graph.create_dynamics_compressor()
    }

    pub fn try_create_dynamics_compressor_with_options(
        &mut self,
        options: DynamicsCompressorOptions,
    ) -> Result<DynamicsCompressorNode, GraphError> {
        self.graph
            .try_create_dynamics_compressor_with_options(options)
    }

    #[must_use]
    pub fn create_analyser(&mut self) -> AnalyserNode {
        self.graph.create_analyser()
    }

    pub fn try_create_analyser_with_options(
        &mut self,
        options: AnalyserOptions,
    ) -> Result<AnalyserNode, GraphError> {
        self.graph.try_create_analyser_with_options(options)
    }

    pub fn try_create_channel_splitter(
        &mut self,
        outputs: usize,
    ) -> Result<ChannelSplitterNode, GraphError> {
        self.graph.try_create_channel_splitter(outputs)
    }

    pub fn try_create_channel_splitter_with_options(
        &mut self,
        options: ChannelSplitterOptions,
    ) -> Result<ChannelSplitterNode, GraphError> {
        self.graph.try_create_channel_splitter_with_options(options)
    }

    #[must_use]
    pub fn create_channel_splitter(&mut self) -> ChannelSplitterNode {
        self.graph.create_channel_splitter()
    }

    pub fn try_create_channel_merger(
        &mut self,
        inputs: usize,
    ) -> Result<ChannelMergerNode, GraphError> {
        self.graph.try_create_channel_merger(inputs)
    }

    pub fn try_create_channel_merger_with_options(
        &mut self,
        options: ChannelMergerOptions,
    ) -> Result<ChannelMergerNode, GraphError> {
        self.graph.try_create_channel_merger_with_options(options)
    }

    #[must_use]
    pub fn create_channel_merger(&mut self) -> ChannelMergerNode {
        self.graph.create_channel_merger()
    }

    #[must_use]
    pub fn create_panner(&mut self) -> PannerNode {
        self.graph.create_panner()
    }

    pub fn try_create_panner_with_options(
        &mut self,
        options: PannerOptions,
    ) -> Result<PannerNode, GraphError> {
        self.graph.try_create_panner_with_options(options)
    }

    pub fn try_create_audio_worklet_node<P>(
        &mut self,
        processor: P,
        options: AudioWorkletNodeOptions,
    ) -> Result<AudioWorkletNode, GraphError>
    where
        P: AudioWorkletProcessor + 'static,
    {
        self.graph.try_create_audio_worklet_node(processor, options)
    }

    #[must_use]
    pub fn create_audio_worklet_node<P>(&mut self, processor: P) -> AudioWorkletNode
    where
        P: AudioWorkletProcessor + 'static,
    {
        self.graph.create_audio_worklet_node(processor)
    }

    pub fn connect(
        &mut self,
        source: impl AudioNodeHandle,
        target: impl AudioNodeHandle,
    ) -> Result<(), GraphError> {
        self.graph.connect(source, target)
    }

    pub fn connect_with_indices(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: impl AudioNodeHandle,
        input: usize,
    ) -> Result<(), GraphError> {
        self.graph
            .connect_with_indices(source, output, target, input)
    }

    pub fn connect_param(
        &mut self,
        source: impl AudioNodeHandle,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.graph.connect_param(source, target)
    }

    pub fn connect_param_from_output(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.graph.connect_param_from_output(source, output, target)
    }

    pub fn disconnect(
        &mut self,
        source: impl AudioNodeHandle,
        target: impl AudioNodeHandle,
    ) -> Result<(), GraphError> {
        self.graph.disconnect(source, target)
    }

    pub fn disconnect_with_indices(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: impl AudioNodeHandle,
        input: usize,
    ) -> Result<(), GraphError> {
        self.graph
            .disconnect_with_indices(source, output, target, input)
    }

    pub fn disconnect_param(
        &mut self,
        source: impl AudioNodeHandle,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.graph.disconnect_param(source, target)
    }

    pub fn disconnect_param_from_output(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.graph
            .disconnect_param_from_output(source, output, target)
    }

    pub fn disconnect_outputs(&mut self, source: impl AudioNodeHandle) -> Result<(), GraphError> {
        self.graph.disconnect_outputs(source)
    }

    pub fn disconnect_param_outputs(
        &mut self,
        source: impl AudioNodeHandle,
    ) -> Result<(), GraphError> {
        self.graph.disconnect_param_outputs(source)
    }

    pub fn node_info(&self, node: impl AudioNodeHandle) -> Result<AudioNodeInfo, GraphError> {
        self.graph.node_info(node)
    }

    pub fn start_rendering(&mut self) -> Result<AudioBuffer, GraphError> {
        if self.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        self.state = OfflineAudioContextState::Running;
        let buffer = self.graph.render_offline_channels(
            self.sample_rate,
            self.length,
            self.number_of_channels,
        )?;
        self.finish_render();
        Ok(buffer)
    }

    fn finish_render(&mut self) {
        self.current_time = self.length as f64 / self.sample_rate as f64;
        self.state = OfflineAudioContextState::Closed;
    }
}
