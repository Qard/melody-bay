#[derive(Debug, Clone)]
pub struct StereoPannerNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StereoPannerOptions {
    pub pan: f32,
}

impl Default for StereoPannerOptions {
    fn default() -> Self {
        Self { pan: 0.0 }
    }
}

impl StereoPannerNode {
    #[must_use]
    pub fn pan(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.pan_param(),
        }
    }

    #[must_use]
    fn pan_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Pan,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "pan" => Some(self.pan()),
            _ => None,
        }
    }
}

impl From<StereoPannerNode> for NodeId {
    fn from(value: StereoPannerNode) -> Self {
        value.id
    }
}

impl From<&StereoPannerNode> for NodeId {
    fn from(value: &StereoPannerNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(StereoPannerNode);

#[derive(Debug, Clone)]
pub struct BiquadFilterHandle {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadFilterOptions {
    pub filter_type: BiquadFilterType,
    pub frequency: f32,
    pub detune: f32,
    pub q: f32,
    pub gain: f32,
}

impl Default for BiquadFilterOptions {
    fn default() -> Self {
        Self {
            filter_type: BiquadFilterType::Lowpass,
            frequency: 350.0,
            detune: 0.0,
            q: 1.0,
            gain: 0.0,
        }
    }
}

impl BiquadFilterHandle {
    #[must_use]
    pub fn type_value(&self) -> BiquadFilterType {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::BiquadFilter { kind, .. } = &inner.nodes[self.id.0].kind {
            *kind
        } else {
            BiquadFilterType::Lowpass
        }
    }

    pub fn set_type(&self, filter_type: BiquadFilterType) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::BiquadFilter {
            kind: node_kind, ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_kind = filter_type;
        }
    }

    #[must_use]
    pub fn frequency(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.frequency_param(),
        }
    }

    #[must_use]
    pub fn detune(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.detune_param(),
        }
    }

    #[must_use]
    pub fn q(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.q_param(),
        }
    }

    #[must_use]
    pub fn gain(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.gain_param(),
        }
    }

    #[must_use]
    fn frequency_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Frequency,
        }
    }

    #[must_use]
    fn detune_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Detune,
        }
    }

    #[must_use]
    fn q_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Q,
        }
    }

    #[must_use]
    fn gain_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::FilterGain,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "frequency" => Some(self.frequency()),
            "detune" => Some(self.detune()),
            "Q" | "q" => Some(self.q()),
            "gain" => Some(self.gain()),
            _ => None,
        }
    }

    pub fn get_frequency_response(
        &self,
        frequency_hz: &[f32],
        mag_response: &mut [f32],
        phase_response: &mut [f32],
    ) -> Result<(), GraphError> {
        if frequency_hz.len() != mag_response.len() || frequency_hz.len() != phase_response.len() {
            return Err(GraphError::InvalidFrequencyResponse);
        }
        let inner = self.graph.lock().expect("graph mutex poisoned");
        let NodeKind::BiquadFilter {
            kind,
            frequency,
            detune,
            q,
            gain,
        } = &inner.nodes[self.id.0].kind
        else {
            return Err(GraphError::UnknownNode);
        };
        let filter_frequency = frequency.value() * 2.0f32.powf(detune.value() / 1200.0);
        let coefficients = BiquadCoefficients::new(
            *kind,
            filter_frequency,
            q.value(),
            gain.value(),
            inner.sample_rate as f64,
        );
        let sample_rate = inner.sample_rate as f32;
        for ((frequency_hz, mag), phase) in frequency_hz
            .iter()
            .zip(mag_response.iter_mut())
            .zip(phase_response.iter_mut())
        {
            let (magnitude, phase_radians) =
                coefficients.frequency_response(*frequency_hz, sample_rate);
            *mag = magnitude;
            *phase = phase_radians;
        }
        Ok(())
    }
}

impl From<BiquadFilterHandle> for NodeId {
    fn from(value: BiquadFilterHandle) -> Self {
        value.id
    }
}

impl From<&BiquadFilterHandle> for NodeId {
    fn from(value: &BiquadFilterHandle) -> Self {
        value.id
    }
}

impl_node_channel_config!(BiquadFilterHandle);

#[derive(Debug, Clone)]
pub struct IirFilterNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IirFilterOptions {
    pub feedforward: Vec<f32>,
    pub feedback: Vec<f32>,
}

impl IirFilterNode {
    pub fn coefficients(
        &self,
        feedforward: impl IntoIterator<Item = f32>,
        feedback: impl IntoIterator<Item = f32>,
    ) -> Result<(), GraphError> {
        let feedforward = feedforward.into_iter().collect::<Vec<_>>();
        let feedback = feedback.into_iter().collect::<Vec<_>>();
        validate_iir_coefficients(&feedforward, &feedback)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::IirFilter {
            feedforward: node_feedforward,
            feedback: node_feedback,
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_feedforward = feedforward;
            *node_feedback = feedback;
        }
        Ok(())
    }

    pub fn get_frequency_response(
        &self,
        frequency_hz: &[f32],
        mag_response: &mut [f32],
        phase_response: &mut [f32],
    ) -> Result<(), GraphError> {
        if frequency_hz.len() != mag_response.len() || frequency_hz.len() != phase_response.len() {
            return Err(GraphError::InvalidFrequencyResponse);
        }
        let inner = self.graph.lock().expect("graph mutex poisoned");
        let NodeKind::IirFilter {
            feedforward,
            feedback,
        } = &inner.nodes[self.id.0].kind
        else {
            return Err(GraphError::UnknownNode);
        };
        let sample_rate = inner.sample_rate as f32;
        for ((frequency_hz, mag), phase) in frequency_hz
            .iter()
            .zip(mag_response.iter_mut())
            .zip(phase_response.iter_mut())
        {
            let (magnitude, phase_radians) =
                iir_frequency_response(feedforward, feedback, *frequency_hz, sample_rate);
            *mag = magnitude;
            *phase = phase_radians;
        }
        Ok(())
    }
}

impl From<IirFilterNode> for NodeId {
    fn from(value: IirFilterNode) -> Self {
        value.id
    }
}

impl From<&IirFilterNode> for NodeId {
    fn from(value: &IirFilterNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(IirFilterNode);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Oversample {
    None,
    TwoX,
    FourX,
}

#[derive(Debug, Clone)]
pub struct WaveShaperNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaveShaperOptions {
    pub curve: Option<Vec<f32>>,
    pub oversample: Oversample,
}

impl Default for WaveShaperOptions {
    fn default() -> Self {
        Self {
            curve: None,
            oversample: Oversample::None,
        }
    }
}

impl WaveShaperNode {
    pub fn try_curve(&self, curve: impl IntoIterator<Item = f32>) -> Result<(), GraphError> {
        let curve = curve.into_iter().collect::<Vec<_>>();
        if curve.is_empty() || curve.iter().any(|sample| !sample.is_finite()) {
            return Err(GraphError::InvalidWaveShaperCurve);
        }
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::WaveShaper {
            curve: node_curve, ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_curve = Some(curve);
        }
        Ok(())
    }

    pub fn clear_curve(&self) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::WaveShaper {
            curve: node_curve, ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_curve = None;
        }
    }

    pub fn set_oversample(&self, oversample: Oversample) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::WaveShaper {
            oversample: node_oversample,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_oversample = oversample;
        }
    }

    #[must_use]
    pub fn curve_value(&self) -> Option<Vec<f32>> {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::WaveShaper { curve, .. } = &inner.nodes[self.id.0].kind {
            curve.clone()
        } else {
            None
        }
    }

    #[must_use]
    pub fn oversample_value(&self) -> Oversample {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::WaveShaper { oversample, .. } = &inner.nodes[self.id.0].kind {
            *oversample
        } else {
            Oversample::None
        }
    }
}

impl From<WaveShaperNode> for NodeId {
    fn from(value: WaveShaperNode) -> Self {
        value.id
    }
}

impl From<&WaveShaperNode> for NodeId {
    fn from(value: &WaveShaperNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(WaveShaperNode);

#[derive(Debug, Clone)]
pub struct ConvolverNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct ConvolverOptions {
    pub buffer: Option<AudioBuffer>,
    pub disable_normalization: bool,
}


impl ConvolverNode {
    pub fn try_buffer(&self, buffer: AudioBuffer) -> Result<(), GraphError> {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if !matches!(buffer.number_of_channels(), 1 | 2 | 4)
            || buffer.length() == 0
            || buffer.sample_rate() != inner.sample_rate
        {
            return Err(GraphError::InvalidConvolverBuffer);
        }
        if let NodeKind::Convolver {
            buffer: node_buffer,
            normalize,
            buffer_normalize,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_buffer = Some(buffer);
            *buffer_normalize = *normalize;
        }
        Ok(())
    }

    pub fn clear_buffer(&self) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Convolver {
            buffer: node_buffer,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_buffer = None;
        }
    }

    pub fn set_normalize(&self, normalize: bool) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Convolver {
            normalize: node_normalize,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_normalize = normalize;
        }
    }

    #[must_use]
    pub fn buffer_value(&self) -> Option<AudioBuffer> {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Convolver { buffer, .. } = &inner.nodes[self.id.0].kind {
            buffer.clone()
        } else {
            None
        }
    }

    #[must_use]
    pub fn normalize_value(&self) -> bool {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Convolver { normalize, .. } = &inner.nodes[self.id.0].kind {
            *normalize
        } else {
            true
        }
    }
}

impl From<ConvolverNode> for NodeId {
    fn from(value: ConvolverNode) -> Self {
        value.id
    }
}

impl From<&ConvolverNode> for NodeId {
    fn from(value: &ConvolverNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(ConvolverNode);

#[derive(Debug, Clone)]
pub struct DelayNodeHandle {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayOptions {
    pub max_delay_time: f64,
    pub delay_time: f32,
}

impl Default for DelayOptions {
    fn default() -> Self {
        Self {
            max_delay_time: 1.0,
            delay_time: 0.0,
        }
    }
}

impl DelayNodeHandle {
    #[must_use]
    pub fn delay_time(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.delay_time_param(),
        }
    }

    #[must_use]
    pub fn delay_time_value(&self) -> f32 {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Delay { delay_time, .. } = &inner.nodes[self.id.0].kind {
            delay_time.value()
        } else {
            0.0
        }
    }

    #[must_use]
    pub fn max_delay_time_value(&self) -> f32 {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Delay { max_delay_time, .. } = &inner.nodes[self.id.0].kind {
            max_delay_time.unwrap_or(1.0)
        } else {
            1.0
        }
    }

    #[must_use]
    fn delay_time_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::DelayTime,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "delayTime" | "delay_time" => Some(self.delay_time()),
            _ => None,
        }
    }
}

impl From<DelayNodeHandle> for NodeId {
    fn from(value: DelayNodeHandle) -> Self {
        value.id
    }
}

impl From<&DelayNodeHandle> for NodeId {
    fn from(value: &DelayNodeHandle) -> Self {
        value.id
    }
}

impl_node_channel_config!(DelayNodeHandle);

#[derive(Debug, Clone)]
pub struct DynamicsCompressorNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicsCompressorOptions {
    pub threshold: f32,
    pub knee: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
}

impl Default for DynamicsCompressorOptions {
    fn default() -> Self {
        Self {
            threshold: -24.0,
            knee: 30.0,
            ratio: 12.0,
            attack: 0.003,
            release: 0.25,
        }
    }
}

impl DynamicsCompressorNode {
    #[must_use]
    pub fn threshold(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.threshold_param(),
        }
    }

    #[must_use]
    pub fn knee(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.knee_param(),
        }
    }

    #[must_use]
    pub fn ratio(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.ratio_param(),
        }
    }

    #[must_use]
    pub fn attack(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.attack_param(),
        }
    }

    #[must_use]
    pub fn release(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.release_param(),
        }
    }

    #[must_use]
    pub fn reduction(&self) -> f32 {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::DynamicsCompressor { reduction, .. } = &inner.nodes[self.id.0].kind {
            f32::from_bits(reduction.load(Ordering::SeqCst))
        } else {
            0.0
        }
    }

    #[must_use]
    fn threshold_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Threshold,
        }
    }

    #[must_use]
    fn knee_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Knee,
        }
    }

    #[must_use]
    fn ratio_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Ratio,
        }
    }

    #[must_use]
    fn attack_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Attack,
        }
    }

    #[must_use]
    fn release_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Release,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "threshold" => Some(self.threshold()),
            "knee" => Some(self.knee()),
            "ratio" => Some(self.ratio()),
            "attack" => Some(self.attack()),
            "release" => Some(self.release()),
            _ => None,
        }
    }
}

impl From<DynamicsCompressorNode> for NodeId {
    fn from(value: DynamicsCompressorNode) -> Self {
        value.id
    }
}

impl From<&DynamicsCompressorNode> for NodeId {
    fn from(value: &DynamicsCompressorNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(DynamicsCompressorNode);

#[derive(Debug, Clone)]
pub struct AnalyserNode {
    id: NodeId,
    state: Arc<Mutex<AnalyserState>>,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyserOptions {
    pub fft_size: usize,
    pub min_decibels: f32,
    pub max_decibels: f32,
    pub smoothing_time_constant: f32,
}

impl Default for AnalyserOptions {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            min_decibels: -100.0,
            max_decibels: -30.0,
            smoothing_time_constant: 0.8,
        }
    }
}

impl AnalyserNode {
    pub fn try_fft_size(&self, size: usize) -> Result<(), GraphError> {
        if !(32..=32768).contains(&size) || !size.is_power_of_two() {
            return Err(GraphError::InvalidAnalyserConfig);
        }
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .resize(size);
        Ok(())
    }

    pub fn try_min_decibels(&self, decibels: f32) -> Result<(), GraphError> {
        let mut state = self.state.lock().expect("analyser mutex poisoned");
        if !decibels.is_finite() || decibels >= state.max_decibels {
            return Err(GraphError::InvalidAnalyserConfig);
        }
        state.min_decibels = decibels;
        Ok(())
    }

    pub fn try_max_decibels(&self, decibels: f32) -> Result<(), GraphError> {
        let mut state = self.state.lock().expect("analyser mutex poisoned");
        if !decibels.is_finite() || decibels <= state.min_decibels {
            return Err(GraphError::InvalidAnalyserConfig);
        }
        state.max_decibels = decibels;
        Ok(())
    }

    pub fn try_smoothing_time_constant(&self, smoothing: f32) -> Result<(), GraphError> {
        if !(0.0..=1.0).contains(&smoothing) {
            return Err(GraphError::InvalidAnalyserConfig);
        }
        let mut state = self.state.lock().expect("analyser mutex poisoned");
        state.smoothing_time_constant = smoothing;
        state.frequency_dirty = true;
        Ok(())
    }

    #[must_use]
    pub fn fft_size_value(&self) -> usize {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .buffer
            .len()
    }

    #[must_use]
    pub fn frequency_bin_count(&self) -> usize {
        self.fft_size_value() / 2
    }

    #[must_use]
    pub fn min_decibels_value(&self) -> f32 {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .min_decibels
    }

    #[must_use]
    pub fn max_decibels_value(&self) -> f32 {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .max_decibels
    }

    #[must_use]
    pub fn smoothing_time_constant_value(&self) -> f32 {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .smoothing_time_constant
    }

    #[must_use]
    pub fn peak(&self) -> f32 {
        self.state.lock().expect("analyser mutex poisoned").peak()
    }

    #[must_use]
    pub fn rms(&self) -> f32 {
        self.state.lock().expect("analyser mutex poisoned").rms()
    }

    #[must_use]
    pub fn time_domain_data(&self) -> Vec<f32> {
        self.float_time_domain_data()
    }

    #[must_use]
    pub fn float_time_domain_data(&self) -> Vec<f32> {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .time_domain_data()
    }

    #[must_use]
    pub fn byte_time_domain_data(&self) -> Vec<u8> {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .time_domain_data()
            .into_iter()
            .map(sample_to_byte)
            .collect()
    }

    #[must_use]
    pub fn float_frequency_data(&self) -> Vec<f32> {
        self.state
            .lock()
            .expect("analyser mutex poisoned")
            .frequency_data()
    }

    #[must_use]
    pub fn byte_frequency_data(&self) -> Vec<u8> {
        let mut state = self.state.lock().expect("analyser mutex poisoned");
        let min_decibels = state.min_decibels;
        let max_decibels = state.max_decibels;
        state
            .frequency_data()
            .into_iter()
            .map(|decibels| decibels_to_byte(decibels, min_decibels, max_decibels))
            .collect()
    }

    pub fn get_float_time_domain_data(&self, destination: &mut [f32]) {
        copy_available(&self.float_time_domain_data(), destination);
    }

    pub fn get_byte_time_domain_data(&self, destination: &mut [u8]) {
        copy_available(&self.byte_time_domain_data(), destination);
    }

    pub fn get_float_frequency_data(&self, destination: &mut [f32]) {
        copy_available(&self.float_frequency_data(), destination);
    }

    pub fn get_byte_frequency_data(&self, destination: &mut [u8]) {
        copy_available(&self.byte_frequency_data(), destination);
    }
}

fn copy_available<T: Copy>(source: &[T], destination: &mut [T]) {
    let length = source.len().min(destination.len());
    destination[..length].copy_from_slice(&source[..length]);
}

impl From<AnalyserNode> for NodeId {
    fn from(value: AnalyserNode) -> Self {
        value.id
    }
}

impl From<&AnalyserNode> for NodeId {
    fn from(value: &AnalyserNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(AnalyserNode);

#[derive(Debug, Clone)]
pub struct ChannelSplitterNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
    context_identity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSplitterOptions {
    pub number_of_outputs: usize,
}

impl Default for ChannelSplitterOptions {
    fn default() -> Self {
        Self {
            number_of_outputs: 6,
        }
    }
}

impl From<ChannelSplitterNode> for NodeId {
    fn from(value: ChannelSplitterNode) -> Self {
        value.id
    }
}

impl From<&ChannelSplitterNode> for NodeId {
    fn from(value: &ChannelSplitterNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(ChannelSplitterNode);

#[derive(Debug, Clone)]
pub struct ChannelMergerNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
    context_identity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelMergerOptions {
    pub number_of_inputs: usize,
}

impl Default for ChannelMergerOptions {
    fn default() -> Self {
        Self {
            number_of_inputs: 6,
        }
    }
}

impl From<ChannelMergerNode> for NodeId {
    fn from(value: ChannelMergerNode) -> Self {
        value.id
    }
}

impl From<&ChannelMergerNode> for NodeId {
    fn from(value: &ChannelMergerNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(ChannelMergerNode);

#[derive(Debug, Clone)]
pub struct AudioWorkletNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

impl AudioWorkletNode {
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        let param = self.param_id(name)?;
        let graph = self.graph.lock().expect("graph mutex poisoned");
        graph.param(param)?;
        Some(AudioParamHandle {
            graph: self.graph.clone(),
            id: param,
        })
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    fn param_id(&self, name: &str) -> Option<ParamId> {
        let graph = self.graph.lock().expect("graph mutex poisoned");
        let node = graph.nodes.get(self.id.0)?;
        let NodeKind::AudioWorklet { parameters, .. } = &node.kind else {
            return None;
        };
        parameters
            .iter()
            .position(|(parameter_name, _)| parameter_name == name)
            .map(|index| ParamId {
                node: self.id,
                param: ParamKind::WorkletParam(index),
            })
    }
}

impl From<AudioWorkletNode> for NodeId {
    fn from(value: AudioWorkletNode) -> Self {
        value.id
    }
}

impl From<&AudioWorkletNode> for NodeId {
    fn from(value: &AudioWorkletNode) -> Self {
        value.id
    }
}

impl_node_channel_config!(AudioWorkletNode);

#[derive(Debug, Clone, PartialEq)]
pub struct AudioWorkletNodeOptions {
    pub number_of_inputs: usize,
    pub number_of_outputs: usize,
    pub output_channel_count: Option<Vec<usize>>,
    pub parameter_descriptors: Vec<AudioWorkletParameterDescriptor>,
    pub parameter_data: HashMap<String, f32>,
    pub processor_options: HashMap<String, String>,
}

impl Default for AudioWorkletNodeOptions {
    fn default() -> Self {
        Self {
            number_of_inputs: 1,
            number_of_outputs: 1,
            output_channel_count: None,
            parameter_descriptors: Vec::new(),
            parameter_data: HashMap::new(),
            processor_options: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioWorkletParameterDescriptor {
    pub name: String,
    pub default_value: f32,
    pub min_value: f32,
    pub max_value: f32,
    pub automation_rate: AutomationRate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioWorkletProcessContext {
    pub current_time: f64,
    pub sample_dt: f64,
    pub parameters: HashMap<String, f32>,
    pub parameter_values: HashMap<String, Vec<f32>>,
    pub processor_options: HashMap<String, String>,
}

pub trait AudioWorkletProcessor: Send {
    fn process(
        &mut self,
        inputs: &[Vec<Vec<f32>>],
        outputs: &mut [Vec<Vec<f32>>],
        context: AudioWorkletProcessContext,
    ) -> bool;
}

#[derive(Clone)]
struct AudioWorkletProcessorNode {
    processor: Arc<Mutex<Box<dyn AudioWorkletProcessor>>>,
    active: Arc<AtomicBool>,
}

pub(crate) struct AudioWorkletRenderQuantum {
    pub(crate) inputs: Vec<Vec<Vec<f32>>>,
    pub(crate) output_channel_count: Vec<usize>,
    pub(crate) time: f64,
    pub(crate) sample_dt: f64,
    pub(crate) parameters: HashMap<String, f32>,
    pub(crate) parameter_values: HashMap<String, Vec<f32>>,
    pub(crate) processor_options: HashMap<String, String>,
}

impl AudioWorkletProcessorNode {
    fn new(processor: impl AudioWorkletProcessor + 'static) -> Self {
        Self {
            processor: Arc::new(Mutex::new(Box::new(processor))),
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    fn process_quantum(&self, quantum: AudioWorkletRenderQuantum) -> Vec<AudioBus> {
        let output_channels = quantum.output_channel_count.iter().sum::<usize>();
        if !self.active.load(Ordering::SeqCst) {
            return vec![AudioBus::silent(output_channels.max(1)); RENDER_QUANTUM_SIZE_USIZE];
        }
        let mut outputs = quantum
            .output_channel_count
            .iter()
            .map(|channels| vec![vec![0.0; RENDER_QUANTUM_SIZE_USIZE]; (*channels).max(1)])
            .collect::<Vec<_>>();
        let continue_processing = self
            .processor
            .lock()
            .expect("audio worklet processor mutex poisoned")
            .process(
                &quantum.inputs,
                &mut outputs,
                AudioWorkletProcessContext {
                    current_time: quantum.time,
                    sample_dt: quantum.sample_dt,
                    parameters: quantum.parameters,
                    parameter_values: quantum.parameter_values,
                    processor_options: quantum.processor_options,
                },
            );
        if !continue_processing {
            self.active.store(false, Ordering::SeqCst);
        }
        (0..RENDER_QUANTUM_SIZE_USIZE)
            .map(|frame| {
                let channels = outputs
                    .iter()
                    .flat_map(|port| {
                        port.iter()
                            .map(|channel| channel.get(frame).copied().unwrap_or(0.0))
                    })
                    .collect::<Vec<_>>();
                if channels.is_empty() {
                    return AudioBus::silent(1);
                }
                AudioBus::from_channels(channels)
            })
            .collect()
    }
}

impl fmt::Debug for AudioWorkletProcessorNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioWorkletProcessorNode")
            .field("active", &self.active.load(Ordering::SeqCst))
            .finish()
    }
}

#[derive(Debug, Clone)]
struct AnalyserState {
    buffer: Vec<f32>,
    previous_frequency: Vec<f32>,
    frequency_data: Vec<f32>,
    frequency_dirty: bool,
    cursor: usize,
    filled: bool,
    min_decibels: f32,
    max_decibels: f32,
    smoothing_time_constant: f32,
}

impl AnalyserState {
    fn new(size: usize) -> Self {
        let size = size.max(1);
        Self {
            buffer: vec![0.0; size],
            previous_frequency: vec![0.0; size / 2],
            frequency_data: vec![f32::NEG_INFINITY; size / 2],
            frequency_dirty: true,
            cursor: 0,
            filled: false,
            min_decibels: -100.0,
            max_decibels: -30.0,
            smoothing_time_constant: 0.8,
        }
    }

    fn resize(&mut self, size: usize) {
        let min_decibels = self.min_decibels;
        let max_decibels = self.max_decibels;
        let smoothing_time_constant = self.smoothing_time_constant;
        *self = Self::new(size);
        self.min_decibels = min_decibels;
        self.max_decibels = max_decibels;
        self.smoothing_time_constant = smoothing_time_constant;
    }

    fn push_bus(&mut self, bus: &AudioBus) {
        self.push_sample(downmix_bus_to_mono(bus.clone()));
    }

    fn push_sample(&mut self, sample: f32) {
        self.buffer[self.cursor] = sample;
        self.frequency_dirty = true;
        self.cursor = (self.cursor + 1) % self.buffer.len();
        if self.cursor == 0 {
            self.filled = true;
        }
    }

    fn time_domain_data(&self) -> Vec<f32> {
        if self.filled {
            self.buffer[self.cursor..]
                .iter()
                .chain(self.buffer[..self.cursor].iter())
                .copied()
                .collect()
        } else {
            self.buffer.clone()
        }
    }

    fn observed_time_domain_data(&self) -> Vec<f32> {
        if self.filled {
            self.time_domain_data()
        } else {
            self.buffer[..self.cursor].to_vec()
        }
    }

    fn peak(&self) -> f32 {
        self.observed_time_domain_data()
            .into_iter()
            .map(f32::abs)
            .fold(0.0, f32::max)
    }

    fn rms(&self) -> f32 {
        let data = self.observed_time_domain_data();
        if data.is_empty() {
            return 0.0;
        }
        let sum = data.iter().map(|sample| sample * sample).sum::<f32>();
        (sum / data.len() as f32).sqrt()
    }

    fn frequency_data(&mut self) -> Vec<f32> {
        if !self.frequency_dirty {
            return self.frequency_data.clone();
        }
        let data = self.time_domain_data();
        let size = self.buffer.len();
        let bin_count = size / 2;
        if self.previous_frequency.len() != bin_count {
            self.previous_frequency = vec![0.0; bin_count];
        }
        if self.frequency_data.len() != bin_count {
            self.frequency_data = vec![f32::NEG_INFINITY; bin_count];
        }
        if data.is_empty() || bin_count == 0 {
            return Vec::new();
        }

        let smoothing = self.smoothing_time_constant;
        let windowed = data
            .iter()
            .enumerate()
            .map(|(index, sample)| *sample * blackman_window(index, size))
            .collect::<Vec<_>>();
        let mut bins = Vec::with_capacity(bin_count);
        for bin in 0..bin_count {
            let mut real = 0.0;
            let mut imag = 0.0;
            for (index, sample) in windowed.iter().enumerate() {
                let angle = TAU * bin as f32 * index as f32 / size as f32;
                real += sample * angle.cos();
                imag -= sample * angle.sin();
            }
            let magnitude = (real.mul_add(real, imag * imag)).sqrt() / size as f32;
            let previous = self.previous_frequency[bin];
            let smoothed = previous * smoothing + magnitude * (1.0 - smoothing);
            self.previous_frequency[bin] = smoothed;
            bins.push(20.0 * smoothed.max(1.0e-12).log10());
        }
        self.frequency_data = bins;
        self.frequency_dirty = false;
        self.frequency_data.clone()
    }
}

fn blackman_window(index: usize, size: usize) -> f32 {
    if size == 0 {
        return 0.0;
    }
    let phase = TAU * index as f32 / size as f32;
    0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos()
}

fn sample_to_byte(sample: f32) -> u8 {
    (128.0 * (1.0 + sample.clamp(-1.0, 1.0)))
        .floor()
        .clamp(0.0, 255.0) as u8
}

fn decibels_to_byte(decibels: f32, min_decibels: f32, max_decibels: f32) -> u8 {
    if max_decibels <= min_decibels {
        return 0;
    }
    (((decibels - min_decibels) / (max_decibels - min_decibels)).clamp(0.0, 1.0) * 255.0).floor()
        as u8
}

#[cfg(test)]
mod analyser_byte_conversion_tests {
    use super::*;

    #[test]
    fn decibels_to_byte_floors_scaled_frequency_bins() {
        assert_eq!(decibels_to_byte(-65.0, -100.0, -30.0), 127);
    }
}
