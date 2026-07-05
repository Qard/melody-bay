#[derive(Debug, Clone)]
pub struct AudioContext {
    inner: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AudioContextOptions {
    pub sample_rate: Option<u32>,
    pub latency_hint: Option<AudioContextLatencyHint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioContextLatencyHint {
    Interactive,
    Balanced,
    Playback,
    Seconds(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioTimestamp {
    pub context_time: f64,
    pub performance_time: f64,
}

#[derive(Debug, Clone)]
pub struct AudioListener {
    graph: Arc<Mutex<GraphInner>>,
}

impl AudioListener {
    #[must_use]
    pub fn position_x(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::PositionX)
    }

    #[must_use]
    pub fn position_y(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::PositionY)
    }

    #[must_use]
    pub fn position_z(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::PositionZ)
    }

    #[must_use]
    pub fn forward_x(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::ForwardX)
    }

    #[must_use]
    pub fn forward_y(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::ForwardY)
    }

    #[must_use]
    pub fn forward_z(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::ForwardZ)
    }

    #[must_use]
    pub fn up_x(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::UpX)
    }

    #[must_use]
    pub fn up_y(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::UpY)
    }

    #[must_use]
    pub fn up_z(&self) -> AudioParamHandle {
        self.param_handle(ParamKind::UpZ)
    }

    #[must_use]
    pub fn position_value(&self) -> [f32; 3] {
        self.graph
            .lock()
            .expect("graph mutex poisoned")
            .listener
            .position_value()
    }

    #[must_use]
    pub fn forward_value(&self) -> [f32; 3] {
        self.graph
            .lock()
            .expect("graph mutex poisoned")
            .listener
            .forward_value()
    }

    #[must_use]
    pub fn up_value(&self) -> [f32; 3] {
        self.graph
            .lock()
            .expect("graph mutex poisoned")
            .listener
            .up_value()
    }

    fn param_handle(&self, param: ParamKind) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: ParamId {
                node: LISTENER_PARAM_NODE,
                param,
            },
        }
    }
}

impl AudioContext {
    #[must_use]
    pub fn new() -> Self {
        Self::try_new_with_options(AudioContextOptions::default())
            .expect("default AudioContext options are valid")
    }

    pub fn try_new_with_sample_rate(sample_rate: u32) -> Result<Self, GraphError> {
        Self::try_new_with_options(AudioContextOptions {
            sample_rate: Some(sample_rate),
            ..Default::default()
        })
    }

    pub fn try_new_with_options(options: AudioContextOptions) -> Result<Self, GraphError> {
        let sample_rate = options.sample_rate.unwrap_or(44_100);
        if !(3_000..=384_000).contains(&sample_rate) {
            return Err(GraphError::InvalidAudioBuffer);
        }
        if let Some(AudioContextLatencyHint::Seconds(seconds)) = options.latency_hint
            && (!seconds.is_finite() || seconds < 0.0) {
                return Err(GraphError::InvalidAutomationValue);
            }
        Ok(Self::with_sample_rate_and_destination_channels(
            sample_rate,
            2,
            options.latency_hint,
        ))
    }

    fn with_sample_rate_and_destination_channels(
        sample_rate: u32,
        destination_channels: usize,
        latency_hint: Option<AudioContextLatencyHint>,
    ) -> Self {
        let mut inner = GraphInner {
            sample_rate,
            latency_hint,
            ..GraphInner::default()
        };
        inner
            .nodes
            .push(NodeDef::destination(destination_channels.max(1)));
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    #[must_use]
    pub fn destination(&self) -> AudioDestinationNode {
        AudioDestinationNode {
            id: NodeId(0),
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.inner.lock().expect("graph mutex poisoned").sample_rate
    }

    #[must_use]
    pub fn current_time(&self) -> f64 {
        self.inner
            .lock()
            .expect("graph mutex poisoned")
            .current_time
    }

    #[must_use]
    pub fn get_output_timestamp(&self) -> AudioTimestamp {
        let current_time = self.current_time();
        AudioTimestamp {
            context_time: current_time,
            performance_time: current_time,
        }
    }

    #[must_use]
    pub fn render_quantum_size(&self) -> usize {
        RENDER_QUANTUM_SIZE_USIZE
    }

    #[must_use]
    pub fn latency_hint(&self) -> Option<AudioContextLatencyHint> {
        self.inner
            .lock()
            .expect("graph mutex poisoned")
            .latency_hint
    }

    #[must_use]
    pub fn state(&self) -> OfflineAudioContextState {
        self.inner.lock().expect("graph mutex poisoned").state
    }

    #[must_use]
    pub fn base_latency(&self) -> f64 {
        0.0
    }

    #[must_use]
    pub fn output_latency(&self) -> f64 {
        0.0
    }

    pub fn suspend(&mut self) -> Result<(), GraphError> {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        if inner.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        inner.state = OfflineAudioContextState::Suspended;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), GraphError> {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        if inner.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        inner.state = OfflineAudioContextState::Running;
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), GraphError> {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        if inner.state == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        inner.state = OfflineAudioContextState::Closed;
        Ok(())
    }

    pub fn try_create_buffer(
        &self,
        number_of_channels: usize,
        length: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, GraphError> {
        if number_of_channels == 0
            || number_of_channels > 32
            || length == 0
            || !(3_000..=384_000).contains(&sample_rate)
        {
            return Err(GraphError::InvalidAudioBuffer);
        }
        Ok(AudioBuffer::from_channels(
            sample_rate,
            length,
            (0..number_of_channels).map(|_| vec![0.0; length]),
        ))
    }

    pub fn create_buffer(
        &self,
        number_of_channels: usize,
        length: usize,
        sample_rate: u32,
    ) -> Result<AudioBuffer, GraphError> {
        self.try_create_buffer(number_of_channels, length, sample_rate)
    }

    pub fn try_create_buffer_with_options(
        &self,
        options: AudioBufferOptions,
    ) -> Result<AudioBuffer, GraphError> {
        self.try_create_buffer(
            options.number_of_channels,
            options.length,
            options.sample_rate,
        )
    }

    pub fn try_create_periodic_wave(
        &self,
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
    ) -> Result<PeriodicWave, GraphError> {
        self.try_create_periodic_wave_with_options(real, imag, PeriodicWaveOptions::default())
    }

    pub fn create_periodic_wave(
        &self,
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
    ) -> Result<PeriodicWave, GraphError> {
        self.try_create_periodic_wave(real, imag)
    }

    pub fn try_create_periodic_wave_with_options(
        &self,
        real: impl IntoIterator<Item = f32>,
        imag: impl IntoIterator<Item = f32>,
        options: PeriodicWaveOptions,
    ) -> Result<PeriodicWave, GraphError> {
        PeriodicWave::try_new_with_options(real, imag, options)
    }

    #[must_use]
    pub fn listener(&self) -> AudioListener {
        AudioListener {
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    fn oscillator_with_type(&mut self, waveform: Waveform) -> OscillatorNode {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let id = NodeId(inner.nodes.len());
        inner.nodes.push(NodeDef::oscillator(waveform));
        OscillatorNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_oscillator(&mut self) -> OscillatorNode {
        self.oscillator_with_type(Waveform::Sine)
    }

    pub fn try_create_oscillator_with_options(
        &mut self,
        options: OscillatorOptions,
    ) -> Result<OscillatorNode, GraphError> {
        let oscillator = self.create_oscillator();
        match options.oscillator_type {
            OscillatorType::Basic(waveform) => oscillator.set_type(waveform),
            OscillatorType::Custom(wave) => oscillator.set_periodic_wave(wave),
        }
        oscillator.frequency().set_value(options.frequency)?;
        oscillator.detune().set_value(options.detune)?;
        Ok(oscillator)
    }

    #[must_use]
    fn constant_with_offset(&mut self, value: f32) -> ConstantSourceNode {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let id = NodeId(inner.nodes.len());
        inner.nodes.push(NodeDef::constant(value));
        ConstantSourceNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_constant_source(&mut self) -> ConstantSourceNode {
        self.constant_with_offset(1.0)
    }

    pub fn try_create_constant_source_with_options(
        &mut self,
        options: ConstantSourceOptions,
    ) -> Result<ConstantSourceNode, GraphError> {
        let source = self.create_constant_source();
        source.offset().set_value(options.offset)?;
        Ok(source)
    }

    #[must_use]
    fn gain(&mut self) -> GainNode {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let id = NodeId(inner.nodes.len());
        inner.nodes.push(NodeDef::gain());
        GainNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_gain(&mut self) -> GainNode {
        self.gain()
    }

    pub fn try_create_gain_with_options(
        &mut self,
        options: GainOptions,
    ) -> Result<GainNode, GraphError> {
        let gain = self.create_gain();
        gain.gain().set_value(options.gain)?;
        Ok(gain)
    }

    #[must_use]
    pub fn create_buffer_source(&mut self) -> AudioBufferSourceNode {
        let id = self.push_node(NodeDef::new(NodeKind::AudioBufferSource {
            buffer: None,
            buffer_assigned: false,
            acquired_buffer: None,
            playback_rate: ParamTimeline::new(1.0)
                .with_nominal_range(f32::MIN, f32::MAX)
                .with_automation_rate(AutomationRate::KRate),
            detune: ParamTimeline::new(0.0)
                .with_nominal_range(f32::MIN, f32::MAX)
                .with_automation_rate(AutomationRate::KRate),
            looping: false,
            loop_range: None,
            start_time: 0.0,
            stop_time: None,
            start_scheduled: false,
            stop_scheduled: false,
            ended: Arc::new(AtomicBool::new(false)),
            offset: 0.0,
            duration: None,
        }));
        AudioBufferSourceNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    pub fn try_create_buffer_source_with_options(
        &mut self,
        options: AudioBufferSourceOptions,
    ) -> Result<AudioBufferSourceNode, GraphError> {
        let source = self.create_buffer_source();
        if let Some(buffer) = options.buffer {
            source.try_set_buffer(buffer)?;
        }
        source.playback_rate().set_value(options.playback_rate)?;
        source.detune().set_value(options.detune)?;
        source.set_looping(options.looping);
        source.try_loop_range(options.loop_start, options.loop_end)?;
        Ok(source)
    }

    #[must_use]
    pub fn create_sound_data_source<D>(&mut self, data: D) -> SoundDataSourceNode
    where
        D: SoundData + Send + 'static,
        D::Error: fmt::Debug + Send + Sync + 'static,
    {
        let id = self.push_node(NodeDef::new(NodeKind::ExternalSound {
            data: ExternalSoundDataNode::new(data),
            start_time: 0.0,
            stop_time: None,
            start_scheduled: false,
            stop_scheduled: false,
            ended: Arc::new(AtomicBool::new(false)),
        }));
        SoundDataSourceNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    fn stereo_panner(&mut self) -> StereoPannerNode {
        let id = self.push_node(NodeDef::fixed_clamped_max(NodeKind::StereoPanner {
            pan: ParamTimeline::new(0.0).with_nominal_range(-1.0, 1.0),
        }));
        StereoPannerNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_stereo_panner(&mut self) -> StereoPannerNode {
        self.stereo_panner()
    }

    pub fn try_create_stereo_panner_with_options(
        &mut self,
        options: StereoPannerOptions,
    ) -> Result<StereoPannerNode, GraphError> {
        let panner = self.create_stereo_panner();
        panner.pan().set_value(options.pan)?;
        Ok(panner)
    }

    #[must_use]
    fn biquad_filter(&mut self, kind: BiquadFilterType) -> BiquadFilterHandle {
        let id = self.push_node(NodeDef::new(NodeKind::BiquadFilter {
            kind,
            frequency: ParamTimeline::new(350.0),
            detune: ParamTimeline::new(0.0)
                .with_nominal_range(-DETUNE_NOMINAL_LIMIT, DETUNE_NOMINAL_LIMIT),
            q: ParamTimeline::new(1.0),
            gain: ParamTimeline::new(0.0).with_nominal_range(f32::NEG_INFINITY, BIQUAD_GAIN_MAX),
        }));
        BiquadFilterHandle {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_biquad_filter(&mut self) -> BiquadFilterHandle {
        self.biquad_filter(BiquadFilterType::Lowpass)
    }

    pub fn try_create_biquad_filter_with_options(
        &mut self,
        options: BiquadFilterOptions,
    ) -> Result<BiquadFilterHandle, GraphError> {
        let filter = self.create_biquad_filter();
        filter.set_type(options.filter_type);
        filter.frequency().set_value(options.frequency)?;
        filter.detune().set_value(options.detune)?;
        filter.q().set_value(options.q)?;
        filter.gain().set_value(options.gain)?;
        Ok(filter)
    }

    #[must_use]
    fn iir_filter(
        &mut self,
        feedforward: impl IntoIterator<Item = f32>,
        feedback: impl IntoIterator<Item = f32>,
    ) -> IirFilterNode {
        let id = self.push_node(NodeDef::new(NodeKind::IirFilter {
            feedforward: feedforward.into_iter().collect(),
            feedback: feedback.into_iter().collect(),
        }));
        IirFilterNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    pub fn try_create_iir_filter(
        &mut self,
        feedforward: impl IntoIterator<Item = f32>,
        feedback: impl IntoIterator<Item = f32>,
    ) -> Result<IirFilterNode, GraphError> {
        let feedforward = feedforward.into_iter().collect::<Vec<_>>();
        let feedback = feedback.into_iter().collect::<Vec<_>>();
        validate_iir_coefficients(&feedforward, &feedback)?;

        Ok(self.iir_filter(feedforward, feedback))
    }

    pub fn create_iir_filter(
        &mut self,
        feedforward: impl IntoIterator<Item = f32>,
        feedback: impl IntoIterator<Item = f32>,
    ) -> Result<IirFilterNode, GraphError> {
        self.try_create_iir_filter(feedforward, feedback)
    }

    pub fn try_create_iir_filter_with_options(
        &mut self,
        options: IirFilterOptions,
    ) -> Result<IirFilterNode, GraphError> {
        self.try_create_iir_filter(options.feedforward, options.feedback)
    }

    #[must_use]
    fn delay_with_max_delay_time(&mut self, max_delay_time: f64) -> DelayNodeHandle {
        let id = self.push_node(NodeDef::new(NodeKind::Delay {
            delay_time: ParamTimeline::new(0.0).with_nominal_range(0.0, max_delay_time as f32),
            max_delay_time: Some(max_delay_time.max(0.0) as f32),
        }));
        DelayNodeHandle {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_delay(&mut self) -> DelayNodeHandle {
        self.delay_with_max_delay_time(1.0)
    }

    pub fn try_create_delay(&mut self, max_delay_time: f64) -> Result<DelayNodeHandle, GraphError> {
        if !(0.0..180.0).contains(&max_delay_time) || max_delay_time == 0.0 {
            return Err(GraphError::InvalidDelayTime);
        }
        Ok(self.delay_with_max_delay_time(max_delay_time))
    }

    pub fn try_create_delay_with_options(
        &mut self,
        options: DelayOptions,
    ) -> Result<DelayNodeHandle, GraphError> {
        let delay = self.try_create_delay(options.max_delay_time)?;
        delay.delay_time().set_value(options.delay_time)?;
        Ok(delay)
    }

    #[must_use]
    pub fn create_wave_shaper(&mut self) -> WaveShaperNode {
        let id = self.push_node(NodeDef::new(NodeKind::WaveShaper {
            curve: None,
            oversample: Oversample::None,
        }));
        WaveShaperNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    pub fn try_create_wave_shaper_with_options(
        &mut self,
        options: WaveShaperOptions,
    ) -> Result<WaveShaperNode, GraphError> {
        let shaper = self.create_wave_shaper();
        shaper.set_oversample(options.oversample);
        if let Some(curve) = options.curve {
            shaper.try_curve(curve)?;
        }
        Ok(shaper)
    }

    #[must_use]
    fn dynamics_compressor(&mut self) -> DynamicsCompressorNode {
        let id = self.push_node(NodeDef::fixed_clamped_max(NodeKind::DynamicsCompressor {
            threshold: ParamTimeline::new(-24.0)
                .with_nominal_range(-100.0, 0.0)
                .with_automation_rate(AutomationRate::KRate),
            knee: ParamTimeline::new(30.0)
                .with_nominal_range(0.0, 40.0)
                .with_automation_rate(AutomationRate::KRate),
            ratio: ParamTimeline::new(12.0)
                .with_nominal_range(1.0, 20.0)
                .with_automation_rate(AutomationRate::KRate),
            attack: ParamTimeline::new(0.003)
                .with_nominal_range(0.0, 1.0)
                .with_automation_rate(AutomationRate::KRate),
            release: ParamTimeline::new(0.25)
                .with_nominal_range(0.0, 1.0)
                .with_automation_rate(AutomationRate::KRate),
            reduction: Arc::new(AtomicU32::new(0.0f32.to_bits())),
        }));
        DynamicsCompressorNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_dynamics_compressor(&mut self) -> DynamicsCompressorNode {
        self.dynamics_compressor()
    }

    pub fn try_create_dynamics_compressor_with_options(
        &mut self,
        options: DynamicsCompressorOptions,
    ) -> Result<DynamicsCompressorNode, GraphError> {
        let compressor = self.create_dynamics_compressor();
        compressor.threshold().set_value(options.threshold)?;
        compressor.knee().set_value(options.knee)?;
        compressor.ratio().set_value(options.ratio)?;
        compressor.attack().set_value(options.attack)?;
        compressor.release().set_value(options.release)?;
        Ok(compressor)
    }

    #[must_use]
    pub fn create_convolver(&mut self) -> ConvolverNode {
        let id = self.push_node(NodeDef::fixed_clamped_max(NodeKind::Convolver {
            buffer: None,
            normalize: true,
            buffer_normalize: true,
        }));
        ConvolverNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    pub fn try_create_convolver_with_options(
        &mut self,
        options: ConvolverOptions,
    ) -> Result<ConvolverNode, GraphError> {
        let convolver = self.create_convolver();
        convolver.set_normalize(!options.disable_normalization);
        if let Some(buffer) = options.buffer {
            convolver.try_buffer(buffer)?;
        }
        Ok(convolver)
    }

    #[must_use]
    fn analyser(&mut self) -> AnalyserNode {
        let state = Arc::new(Mutex::new(AnalyserState::new(2048)));
        let id = self.push_node(NodeDef::new(NodeKind::Analyser {
            state: Arc::clone(&state),
        }));
        AnalyserNode {
            id,
            state,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_analyser(&mut self) -> AnalyserNode {
        self.analyser()
    }

    pub fn try_create_analyser_with_options(
        &mut self,
        options: AnalyserOptions,
    ) -> Result<AnalyserNode, GraphError> {
        if !(32..=32768).contains(&options.fft_size)
            || !options.fft_size.is_power_of_two()
            || !options.min_decibels.is_finite()
            || !options.max_decibels.is_finite()
            || options.min_decibels >= options.max_decibels
            || !(0.0..=1.0).contains(&options.smoothing_time_constant)
        {
            return Err(GraphError::InvalidAnalyserConfig);
        }
        let analyser = self.create_analyser();
        {
            let mut state = analyser.state.lock().expect("analyser mutex poisoned");
            state.resize(options.fft_size);
            state.min_decibels = options.min_decibels;
            state.max_decibels = options.max_decibels;
            state.smoothing_time_constant = options.smoothing_time_constant;
            state.frequency_dirty = true;
        }
        Ok(analyser)
    }

    #[must_use]
    fn channel_splitter(&mut self, outputs: usize) -> ChannelSplitterNode {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let id = NodeId(inner.nodes.len());
        inner.nodes.push(NodeDef::channel_splitter(outputs));
        ChannelSplitterNode {
            id,
            graph: Arc::clone(&self.inner),
            context_identity: self.context_identity(),
        }
    }

    pub fn try_create_channel_splitter(
        &mut self,
        outputs: usize,
    ) -> Result<ChannelSplitterNode, GraphError> {
        if !(1..=32).contains(&outputs) {
            return Err(GraphError::InvalidChannelCount);
        }
        Ok(self.channel_splitter(outputs))
    }

    pub fn try_create_channel_splitter_with_options(
        &mut self,
        options: ChannelSplitterOptions,
    ) -> Result<ChannelSplitterNode, GraphError> {
        self.try_create_channel_splitter(options.number_of_outputs)
    }

    #[must_use]
    pub fn create_channel_splitter(&mut self) -> ChannelSplitterNode {
        self.channel_splitter(6)
    }

    #[must_use]
    fn channel_merger(&mut self, inputs: usize) -> ChannelMergerNode {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let id = NodeId(inner.nodes.len());
        inner.nodes.push(NodeDef::channel_merger(inputs));
        ChannelMergerNode {
            id,
            graph: Arc::clone(&self.inner),
            context_identity: self.context_identity(),
        }
    }

    pub fn try_create_channel_merger(
        &mut self,
        inputs: usize,
    ) -> Result<ChannelMergerNode, GraphError> {
        if !(1..=32).contains(&inputs) {
            return Err(GraphError::InvalidChannelCount);
        }
        Ok(self.channel_merger(inputs))
    }

    pub fn try_create_channel_merger_with_options(
        &mut self,
        options: ChannelMergerOptions,
    ) -> Result<ChannelMergerNode, GraphError> {
        self.try_create_channel_merger(options.number_of_inputs)
    }

    #[must_use]
    pub fn create_channel_merger(&mut self) -> ChannelMergerNode {
        self.channel_merger(6)
    }

    #[must_use]
    fn panner(&mut self) -> PannerNode {
        let id = self.push_node(NodeDef::fixed_clamped_max(NodeKind::Panner {
            position_x: ParamTimeline::new(0.0),
            position_y: ParamTimeline::new(0.0),
            position_z: ParamTimeline::new(0.0),
            orientation_x: ParamTimeline::new(1.0),
            orientation_y: ParamTimeline::new(0.0),
            orientation_z: ParamTimeline::new(0.0),
            panning_model: PanningModel::EqualPower,
            distance_model: DistanceModel::Inverse,
            ref_distance: 1.0,
            max_distance: 10_000.0,
            rolloff_factor: 1.0,
            cone_inner_angle: 360.0,
            cone_outer_angle: 360.0,
            cone_outer_gain: 0.0,
        }));
        PannerNode {
            id,
            graph: Arc::clone(&self.inner),
        }
    }

    #[must_use]
    pub fn create_panner(&mut self) -> PannerNode {
        self.panner()
    }

    pub fn try_create_panner_with_options(
        &mut self,
        options: PannerOptions,
    ) -> Result<PannerNode, GraphError> {
        let panner = self.create_panner();
        panner.set_panning_model(options.panning_model)?;
        panner.set_distance_model(options.distance_model);
        panner.position_x().set_value(options.position_x)?;
        panner.position_y().set_value(options.position_y)?;
        panner.position_z().set_value(options.position_z)?;
        panner.orientation_x().set_value(options.orientation_x)?;
        panner.orientation_y().set_value(options.orientation_y)?;
        panner.orientation_z().set_value(options.orientation_z)?;
        panner.try_ref_distance(options.ref_distance)?;
        panner.try_max_distance(options.max_distance)?;
        panner.try_rolloff_factor(options.rolloff_factor)?;
        panner.try_cone_inner_angle(options.cone_inner_angle)?;
        panner.try_cone_outer_angle(options.cone_outer_angle)?;
        panner.try_cone_outer_gain(options.cone_outer_gain)?;
        Ok(panner)
    }

    pub fn try_create_audio_worklet_node<P>(
        &mut self,
        processor: P,
        options: AudioWorkletNodeOptions,
    ) -> Result<AudioWorkletNode, GraphError>
    where
        P: AudioWorkletProcessor + 'static,
    {
        let explicit_output_channel_count = options.output_channel_count.clone();
        let output_channel_count = if explicit_output_channel_count.is_none()
            && options.number_of_inputs == 1
            && options.number_of_outputs == 1
        {
            None
        } else {
            Some(
                explicit_output_channel_count
                    .clone()
                    .unwrap_or_else(|| vec![1; options.number_of_outputs]),
            )
        };
        if options.number_of_inputs > 32
            || options.number_of_outputs > 32
            || (options.number_of_inputs == 0 && options.number_of_outputs == 0)
            || options
                .output_channel_count
                .as_ref()
                .is_some_and(|counts| counts.len() != options.number_of_outputs)
            || output_channel_count
                .as_ref()
                .is_some_and(|counts| counts.iter().any(|count| !(1..=32).contains(count)))
            || !validate_audio_worklet_parameters(
                &options.parameter_descriptors,
                &options.parameter_data,
            )
        {
            return Err(GraphError::InvalidAudioWorkletOptions);
        }
        let parameters = options
            .parameter_descriptors
            .iter()
            .map(|descriptor| {
                let initial_value = options
                    .parameter_data
                    .get(&descriptor.name)
                    .copied()
                    .unwrap_or(descriptor.default_value);
                (
                    descriptor.name.clone(),
                    ParamTimeline::new(initial_value)
                        .with_nominal_range(descriptor.min_value, descriptor.max_value)
                        .with_automation_rate(descriptor.automation_rate),
                )
            })
            .collect();
        let id = self.push_node(NodeDef::new(NodeKind::AudioWorklet {
            inputs: options.number_of_inputs,
            outputs: options.number_of_outputs,
            output_channel_count,
            parameters,
            processor_options: options.processor_options,
            processor: AudioWorkletProcessorNode::new(processor),
        }));
        Ok(AudioWorkletNode {
            id,
            graph: self.inner.clone(),
        })
    }

    #[must_use]
    pub fn create_audio_worklet_node<P>(&mut self, processor: P) -> AudioWorkletNode
    where
        P: AudioWorkletProcessor + 'static,
    {
        self.try_create_audio_worklet_node(processor, AudioWorkletNodeOptions::default())
            .expect("default audio worklet options are valid")
    }

    fn push_node(&mut self, node: NodeDef) -> NodeId {
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let id = NodeId(inner.nodes.len());
        inner.nodes.push(node);
        id
    }

    fn context_identity(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    fn validate_handle_context(&self, handle: &impl AudioNodeHandle) -> Result<(), GraphError> {
        if handle
            .context_identity()
            .is_some_and(|identity| identity != self.context_identity())
        {
            return Err(GraphError::WrongContext);
        }
        Ok(())
    }

    fn validate_param_target_context(&self, target: &AudioParamHandle) -> Result<(), GraphError> {
        if target
            .context_identity()
            .is_some_and(|identity| identity != self.context_identity())
        {
            return Err(GraphError::WrongContext);
        }
        Ok(())
    }

    pub fn label_node(
        &mut self,
        node: impl AudioNodeHandle,
        label: impl Into<String>,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&node)?;
        let node = node.node_id();
        let label = label.into();
        if label.is_empty() || label.contains('.') || label.contains('#') {
            return Err(GraphError::InvalidNodeLabel);
        }
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.validate_node(node)?;
        if inner
            .nodes
            .iter()
            .enumerate()
            .any(|(index, existing)| index != node.0 && existing.label.as_deref() == Some(&label))
        {
            return Err(GraphError::InvalidNodeLabel);
        }
        inner.nodes[node.0].label = Some(label);
        Ok(())
    }

    pub fn connect(
        &mut self,
        source: impl AudioNodeHandle,
        target: impl AudioNodeHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_handle_context(&target)?;
        let source = source.node_id();
        let target = target.node_id();
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.connect_nodes(source, target)
    }

    pub fn connect_with_indices(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: impl AudioNodeHandle,
        input: usize,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_handle_context(&target)?;
        let source = source.node_id();
        let target = target.node_id();
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.connect_nodes_with_indices(source, output, target, input)
    }

    pub fn connect_param(
        &mut self,
        source: impl AudioNodeHandle,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_param_target_context(&target)?;
        let source = source.node_id();
        let target = target.id;
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.connect_param_node(source, target)
    }

    pub fn connect_param_from_output(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_param_target_context(&target)?;
        let source = source.node_id();
        let target = target.id;
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.connect_param_node_from_output(source, output, target)
    }

    pub fn disconnect(
        &mut self,
        source: impl AudioNodeHandle,
        target: impl AudioNodeHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_handle_context(&target)?;
        let source = source.node_id();
        let target = target.node_id();
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.disconnect_nodes(source, target)
    }

    pub fn disconnect_with_indices(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: impl AudioNodeHandle,
        input: usize,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_handle_context(&target)?;
        let source = source.node_id();
        let target = target.node_id();
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.disconnect_nodes_with_indices(source, output, target, input)
    }

    pub fn disconnect_param(
        &mut self,
        source: impl AudioNodeHandle,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_param_target_context(&target)?;
        let source = source.node_id();
        let target = target.id;
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.disconnect_param_node(source, target)
    }

    pub fn disconnect_param_from_output(
        &mut self,
        source: impl AudioNodeHandle,
        output: usize,
        target: AudioParamHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        self.validate_param_target_context(&target)?;
        let source = source.node_id();
        let target = target.id;
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.disconnect_param_node_from_output(source, output, target)
    }

    pub fn disconnect_outputs(&mut self, source: impl AudioNodeHandle) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        let source = source.node_id();
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.validate_node(source)?;
        inner
            .connections
            .retain(|connection| connection.source != source);
        Ok(())
    }

    pub fn disconnect_param_outputs(
        &mut self,
        source: impl AudioNodeHandle,
    ) -> Result<(), GraphError> {
        self.validate_handle_context(&source)?;
        let source = source.node_id();
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        inner.validate_node(source)?;
        inner
            .param_connections
            .retain(|connection| connection.source != source);
        Ok(())
    }

    #[must_use]
    pub fn sound_data(&self) -> AudioContextSoundData {
        AudioContextSoundData {
            graph: self.clone(),
            sample_rate: self.sample_rate(),
        }
    }

    pub(crate) fn render_offline_channels(
        &self,
        sample_rate: u32,
        frames: usize,
        channels: usize,
    ) -> Result<AudioBuffer, GraphError> {
        if self.state() == OfflineAudioContextState::Closed {
            return Err(GraphError::ContextClosed);
        }
        let sample_rate = sample_rate.max(1);
        let channels = channels.max(1);
        let mut compiled = self.compiled()?;
        compiled.set_destination_channel_count(channels);
        let mut runtime = compiled.runtime()?;
        let info = MockInfoBuilder::new().build();
        let sample_dt = 1.0 / sample_rate as f64;
        let mut rendered = vec![vec![0.0; frames]; channels];
        let mut frame_index = 0;
        while frame_index < frames {
            let quantum_frames = (frames - frame_index).min(RENDER_QUANTUM_SIZE_USIZE);
            let quantum_start = frame_index as f64 * sample_dt;
            let quantum = compiled.render_bus_quantum_with_runtime(
                RenderQuantum {
                    start: quantum_start,
                    global_start: quantum_start,
                    sample_dt,
                    frames: quantum_frames,
                    commit_source_state: false,
                },
                None,
                &mut runtime,
                &info,
            );
            for (offset, frame) in quantum.iter().enumerate() {
                let sample_index = frame_index + offset;
                for (channel, samples) in rendered.iter_mut().enumerate() {
                    samples[sample_index] = frame.channel(channel);
                }
            }
            frame_index += quantum_frames;
        }
        Ok(AudioBuffer::from_channels(sample_rate, frames, rendered))
    }

    pub fn node_info(&self, node: impl AudioNodeHandle) -> Result<AudioNodeInfo, GraphError> {
        self.validate_handle_context(&node)?;
        let node = node.node_id();
        let inner = self.inner.lock().expect("graph mutex poisoned");
        inner.validate_node(node)?;
        Ok(inner.nodes[node.0].info())
    }

    fn compiled(&self) -> Result<CompiledGraph, GraphError> {
        let inner = self.inner.lock().expect("graph mutex poisoned");
        inner.compile()
    }

    fn schedule_named_param_automation(&mut self, automation: &TimedAutomationEvent) {
        if validate_automation_time(automation.time_seconds).is_err() {
            return;
        }
        let Some(target) = sequencer_param_target(&automation.target) else {
            return;
        };
        let mut inner = self.inner.lock().expect("graph mutex poisoned");
        let mut matched = 0usize;
        for node in &mut inner.nodes {
            if !target.matches_label(node.label.as_deref()) {
                continue;
            }
            let Some(timeline) = node.kind.param_mut(target.param) else {
                continue;
            };
            let should_apply = target
                .index
                .is_none_or(|target_index| target_index == matched);
            matched += 1;
            if !should_apply {
                continue;
            }
            if timeline
                .validate_event_time_for_value_curves(automation.time_seconds)
                .is_err()
            {
                continue;
            }
            match &automation.shape {
                AutomationShape::SetValue { value } => {
                    if validate_automation_value(*value).is_ok() {
                        *timeline = timeline
                            .clone()
                            .with_time_domain(ParamTimeDomain::Global)
                            .set_value_at_time(*value, automation.time_seconds);
                    }
                }
                AutomationShape::LinearRamp { value } => {
                    if validate_automation_value(*value).is_ok() {
                        *timeline = timeline
                            .clone()
                            .with_time_domain(ParamTimeDomain::Global)
                            .linear_ramp_to_value_at_time(*value, automation.time_seconds);
                    }
                }
                AutomationShape::ValueCurve {
                    values,
                    duration_seconds,
                } => {
                    if values
                        .iter()
                        .copied()
                        .all(|value| validate_automation_value(value).is_ok())
                    {
                        *timeline = timeline
                            .clone()
                            .with_time_domain(ParamTimeDomain::Global)
                            .set_value_curve_at_time(
                                values.iter().copied(),
                                automation.time_seconds,
                                *duration_seconds,
                            );
                    }
                }
            }
        }
    }

    fn validate_sequencer_automation_target(
        &self,
        track_id: &TrackId,
        automation: &TimedAutomationEvent,
    ) -> Result<(), SequencerValidationError> {
        validate_automation_time(automation.time_seconds).map_err(|_| {
            SequencerValidationError::InvalidAutomationTime {
                track_id: track_id.clone(),
                target: automation.target.clone(),
                time_seconds: automation.time_seconds,
            }
        })?;
        validate_automation_shape(track_id, automation)?;
        let Some(target) = sequencer_param_target(&automation.target) else {
            return Err(SequencerValidationError::InvalidAutomationTarget {
                track_id: track_id.clone(),
                target: automation.target.clone(),
            });
        };
        let inner = self.inner.lock().expect("graph mutex poisoned");
        let matches = inner
            .nodes
            .iter()
            .filter(|node| {
                target.matches_label(node.label.as_deref())
                    && node.kind.param(target.param).is_some()
            })
            .count();
        let valid = target
            .index
            .map_or(matches > 0, |target_index| target_index < matches);
        if valid {
            Ok(())
        } else {
            Err(SequencerValidationError::InvalidAutomationTarget {
                track_id: track_id.clone(),
                target: automation.target.clone(),
            })
        }
    }
}
