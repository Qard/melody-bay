#[derive(Debug, Clone)]
pub struct OscillatorNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OscillatorType {
    Basic(Waveform),
    Custom(PeriodicWave),
}

impl Default for OscillatorType {
    fn default() -> Self {
        Self::Basic(Waveform::Sine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OscillatorTypeValue {
    Sine,
    Square,
    Sawtooth,
    Triangle,
    Custom,
}

impl From<Waveform> for OscillatorTypeValue {
    fn from(value: Waveform) -> Self {
        match value {
            Waveform::Sine => Self::Sine,
            Waveform::Square => Self::Square,
            Waveform::Sawtooth => Self::Sawtooth,
            Waveform::Triangle => Self::Triangle,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OscillatorOptions {
    pub oscillator_type: OscillatorType,
    pub frequency: f32,
    pub detune: f32,
}

impl Default for OscillatorOptions {
    fn default() -> Self {
        Self {
            oscillator_type: OscillatorType::default(),
            frequency: 440.0,
            detune: 0.0,
        }
    }
}

impl OscillatorNode {
    #[must_use]
    pub fn type_value(&self) -> Waveform {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator { waveform, .. } = &inner.nodes[self.id.0].kind {
            *waveform
        } else {
            Waveform::Sine
        }
    }

    #[must_use]
    pub fn oscillator_type(&self) -> OscillatorTypeValue {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator {
            waveform,
            periodic_wave,
            ..
        } = &inner.nodes[self.id.0].kind
        {
            if periodic_wave.is_some() {
                OscillatorTypeValue::Custom
            } else {
                (*waveform).into()
            }
        } else {
            OscillatorTypeValue::Sine
        }
    }

    #[must_use]
    pub fn frequency_value(&self) -> f32 {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator { frequency, .. } = &inner.nodes[self.id.0].kind {
            frequency.value()
        } else {
            0.0
        }
    }

    pub fn set_type(&self, waveform: Waveform) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator {
            waveform: node_waveform,
            periodic_wave,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_waveform = waveform;
            *periodic_wave = None;
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
    fn frequency_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Frequency,
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
    fn detune_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Detune,
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
            _ => None,
        }
    }

    pub fn set_periodic_wave(&self, wave: PeriodicWave) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator { periodic_wave, .. } = &mut inner.nodes[self.id.0].kind {
            *periodic_wave = Some(wave);
        }
    }

    #[must_use]
    pub fn ended(&self) -> bool {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator { ended, .. } = &inner.nodes[self.id.0].kind {
            ended.load(Ordering::SeqCst)
        } else {
            false
        }
    }

    pub fn try_start(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator {
            start_time,
            start_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if *start_scheduled {
                return Err(GraphError::SourceAlreadyStarted);
            }
            *start_time = time.max(0.0);
            *start_scheduled = true;
            ended.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn try_stop(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Oscillator {
            stop_time,
            start_scheduled,
            stop_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if !*start_scheduled {
                return Err(GraphError::SourceNotStarted);
            }
            if ended.load(Ordering::SeqCst) {
                return Ok(());
            }
            let time = time.max(0.0);
            *stop_time = Some(time);
            *stop_scheduled = true;
        }
        Ok(())
    }
}

impl From<OscillatorNode> for NodeId {
    fn from(value: OscillatorNode) -> Self {
        value.id
    }
}

impl From<&OscillatorNode> for NodeId {
    fn from(value: &OscillatorNode) -> Self {
        value.id
    }
}

#[derive(Debug, Clone)]
pub struct ConstantSourceNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConstantSourceOptions {
    pub offset: f32,
}

impl Default for ConstantSourceOptions {
    fn default() -> Self {
        Self { offset: 1.0 }
    }
}

impl ConstantSourceNode {
    #[must_use]
    pub fn offset(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.offset_param(),
        }
    }

    #[must_use]
    fn offset_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Offset,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "offset" => Some(self.offset()),
            _ => None,
        }
    }

    #[must_use]
    pub fn ended(&self) -> bool {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Constant { ended, .. } = &inner.nodes[self.id.0].kind {
            ended.load(Ordering::SeqCst)
        } else {
            false
        }
    }

    pub fn try_start(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Constant {
            start_time,
            start_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if *start_scheduled {
                return Err(GraphError::SourceAlreadyStarted);
            }
            *start_time = time.max(0.0);
            *start_scheduled = true;
            ended.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn try_stop(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::Constant {
            stop_time,
            start_scheduled,
            stop_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if !*start_scheduled {
                return Err(GraphError::SourceNotStarted);
            }
            if ended.load(Ordering::SeqCst) {
                return Ok(());
            }
            let time = time.max(0.0);
            *stop_time = Some(time);
            *stop_scheduled = true;
        }
        Ok(())
    }
}

impl From<ConstantSourceNode> for NodeId {
    fn from(value: ConstantSourceNode) -> Self {
        value.id
    }
}

impl From<&ConstantSourceNode> for NodeId {
    fn from(value: &ConstantSourceNode) -> Self {
        value.id
    }
}

#[derive(Debug, Clone)]
pub struct AudioBufferSourceNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioBufferSourceOptions {
    pub buffer: Option<AudioBuffer>,
    pub playback_rate: f32,
    pub detune: f32,
    pub looping: bool,
    pub loop_start: f64,
    pub loop_end: f64,
}

impl Default for AudioBufferSourceOptions {
    fn default() -> Self {
        Self {
            buffer: None,
            playback_rate: 1.0,
            detune: 0.0,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.0,
        }
    }
}

impl AudioBufferSourceNode {
    pub fn try_set_buffer(&self, buffer: AudioBuffer) -> Result<(), GraphError> {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource {
            buffer: node_buffer,
            buffer_assigned,
            acquired_buffer,
            start_scheduled,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if *buffer_assigned {
                return Err(GraphError::InvalidState);
            }
            *buffer_assigned = true;
            *node_buffer = Some(buffer.clone());
            if *start_scheduled {
                *acquired_buffer = Some(buffer);
            }
        }
        Ok(())
    }

    pub fn clear_buffer(&self) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource {
            buffer: node_buffer,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            *node_buffer = None;
        }
    }

    #[must_use]
    pub fn buffer_value(&self) -> Option<AudioBuffer> {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { buffer, .. } = &inner.nodes[self.id.0].kind {
            buffer.clone()
        } else {
            None
        }
    }

    #[must_use]
    pub fn playback_rate(&self) -> AudioParamHandle {
        AudioParamHandle {
            graph: Arc::clone(&self.graph),
            id: self.playback_rate_param(),
        }
    }

    #[must_use]
    fn playback_rate_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::PlaybackRate,
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
    fn detune_param(&self) -> ParamId {
        ParamId {
            node: self.id,
            param: ParamKind::Detune,
        }
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<AudioParamHandle> {
        self.parameter(name)
    }

    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<AudioParamHandle> {
        match name {
            "playbackRate" | "playback_rate" => Some(self.playback_rate()),
            "detune" => Some(self.detune()),
            _ => None,
        }
    }

    pub fn try_loop_range(&self, start_seconds: f64, end_seconds: f64) -> Result<(), GraphError> {
        if !start_seconds.is_finite() || !end_seconds.is_finite() {
            return Err(GraphError::InvalidLoopRange);
        }
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { loop_range, .. } = &mut inner.nodes[self.id.0].kind {
            *loop_range = Some((start_seconds, end_seconds));
        }
        Ok(())
    }

    pub fn try_loop_start(&self, start_seconds: f64) -> Result<(), GraphError> {
        if !start_seconds.is_finite() {
            return Err(GraphError::InvalidLoopRange);
        }
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { loop_range, .. } = &mut inner.nodes[self.id.0].kind {
            let end_seconds = loop_range.map(|(_, end)| end).unwrap_or(0.0);
            *loop_range = Some((start_seconds, end_seconds));
        }
        Ok(())
    }

    pub fn try_loop_end(&self, end_seconds: f64) -> Result<(), GraphError> {
        if !end_seconds.is_finite() {
            return Err(GraphError::InvalidLoopRange);
        }
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { loop_range, .. } = &mut inner.nodes[self.id.0].kind {
            let start_seconds = loop_range.map(|(start, _)| start).unwrap_or(0.0);
            *loop_range = Some((start_seconds, end_seconds));
        }
        Ok(())
    }

    pub fn set_looping(&self, enabled: bool) {
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { looping, .. } = &mut inner.nodes[self.id.0].kind {
            *looping = enabled;
        }
    }

    #[must_use]
    pub fn looping_value(&self) -> bool {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { looping, .. } = &inner.nodes[self.id.0].kind {
            *looping
        } else {
            false
        }
    }

    #[must_use]
    pub fn loop_range_value(&self) -> Option<(f64, f64)> {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { loop_range, .. } = &inner.nodes[self.id.0].kind {
            *loop_range
        } else {
            None
        }
    }

    #[must_use]
    pub fn loop_start_value(&self) -> f64 {
        self.loop_range_value()
            .map(|(start_seconds, _)| start_seconds)
            .unwrap_or(0.0)
    }

    #[must_use]
    pub fn loop_end_value(&self) -> f64 {
        self.loop_range_value()
            .map(|(_, end_seconds)| end_seconds)
            .unwrap_or(0.0)
    }

    #[must_use]
    pub fn ended(&self) -> bool {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource { ended, .. } = &inner.nodes[self.id.0].kind {
            ended.load(Ordering::SeqCst)
        } else {
            false
        }
    }

    pub fn try_start(&self, time: f64) -> Result<(), GraphError> {
        self.try_start_with_offset(time, 0.0)
    }

    pub fn try_start_with_offset(&self, time: f64, offset: f64) -> Result<(), GraphError> {
        self.try_set_start(time, offset, None)
    }

    pub fn try_start_with_offset_and_duration(
        &self,
        time: f64,
        offset: f64,
        duration: f64,
    ) -> Result<(), GraphError> {
        self.try_set_start(time, offset, Some(duration))
    }

    fn try_set_start(
        &self,
        time: f64,
        source_offset: f64,
        source_duration: Option<f64>,
    ) -> Result<(), GraphError> {
        validate_source_time(time)?;
        validate_source_time(source_offset)?;
        if let Some(duration) = source_duration {
            validate_source_time(duration)?;
        }
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource {
            buffer,
            acquired_buffer,
            start_time,
            start_scheduled,
            ended,
            offset,
            duration,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if *start_scheduled {
                return Err(GraphError::SourceAlreadyStarted);
            }
            *start_time = time.max(0.0);
            *start_scheduled = true;
            *acquired_buffer = buffer.clone();
            ended.store(false, Ordering::SeqCst);
            *offset = source_offset.max(0.0);
            *duration = source_duration;
        }
        Ok(())
    }

    pub fn try_stop(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::AudioBufferSource {
            stop_time,
            start_scheduled,
            stop_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if !*start_scheduled {
                return Err(GraphError::SourceNotStarted);
            }
            if ended.load(Ordering::SeqCst) {
                return Ok(());
            }
            let time = time.max(0.0);
            *stop_time = Some(time);
            *stop_scheduled = true;
        }
        Ok(())
    }
}

impl From<AudioBufferSourceNode> for NodeId {
    fn from(value: AudioBufferSourceNode) -> Self {
        value.id
    }
}

impl From<&AudioBufferSourceNode> for NodeId {
    fn from(value: &AudioBufferSourceNode) -> Self {
        value.id
    }
}

#[derive(Debug, Clone)]
pub struct SoundDataSourceNode {
    id: NodeId,
    graph: Arc<Mutex<GraphInner>>,
}

impl SoundDataSourceNode {
    #[must_use]
    pub fn ended(&self) -> bool {
        let inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::ExternalSound { ended, .. } = &inner.nodes[self.id.0].kind {
            ended.load(Ordering::SeqCst)
        } else {
            false
        }
    }

    pub fn try_start(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::ExternalSound {
            start_time,
            start_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if *start_scheduled {
                return Err(GraphError::SourceAlreadyStarted);
            }
            *start_time = time.max(0.0);
            *start_scheduled = true;
            ended.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn try_stop(&self, time: f64) -> Result<(), GraphError> {
        validate_source_time(time)?;
        let mut inner = self.graph.lock().expect("graph mutex poisoned");
        if let NodeKind::ExternalSound {
            stop_time,
            start_scheduled,
            stop_scheduled,
            ended,
            ..
        } = &mut inner.nodes[self.id.0].kind
        {
            if !*start_scheduled {
                return Err(GraphError::SourceNotStarted);
            }
            if ended.load(Ordering::SeqCst) {
                return Ok(());
            }
            *stop_time = Some(time.max(0.0));
            *stop_scheduled = true;
        }
        Ok(())
    }
}

impl From<SoundDataSourceNode> for NodeId {
    fn from(value: SoundDataSourceNode) -> Self {
        value.id
    }
}

impl From<&SoundDataSourceNode> for NodeId {
    fn from(value: &SoundDataSourceNode) -> Self {
        value.id
    }
}
