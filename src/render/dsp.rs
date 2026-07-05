fn ensure_graph_outputs(outputs: &mut Vec<AudioBus>, nodes: usize) {
    outputs.resize_with(nodes, || AudioBus::silent(1));
    for output in outputs.iter_mut() {
        output.channels.clear();
        output.channels.push(0.0);
    }
}

#[derive(Debug, Default)]
struct SampleVoiceParamRuntime {
    playback_rate: ParamTimelineRuntime,
    detune: ParamTimelineRuntime,
    envelope_gain: ParamTimelineRuntime,
    channel_gain: ParamTimelineRuntime,
    pan: ParamTimelineRuntime,
}

fn stereo_panner_mono(value: f32, pan: f32) -> Frame {
    let pan = pan.clamp(-1.0, 1.0);
    if pan <= -1.0 {
        return Frame::new(value, 0.0);
    }
    if pan >= 1.0 {
        return Frame::new(0.0, value);
    }
    let angle = (pan + 1.0) * FRAC_PI_4;
    Frame::new(value * angle.cos(), value * angle.sin())
}

fn stereo_panner_frame(input: Frame, pan: f32) -> Frame {
    let pan = pan.clamp(-1.0, 1.0);
    if pan <= -1.0 {
        return Frame::new(input.left + input.right, 0.0);
    }
    if pan >= 1.0 {
        return Frame::new(0.0, input.right + input.left);
    }
    if pan <= 0.0 {
        let angle = (pan + 1.0) * FRAC_PI_2;
        Frame::new(
            input.left + input.right * angle.cos(),
            input.right * angle.sin(),
        )
    } else {
        let angle = pan * FRAC_PI_2;
        Frame::new(
            input.left * angle.cos(),
            input.right + input.left * angle.sin(),
        )
    }
}

fn stereo_panner_bus(input: &AudioBus, pan: f32) -> AudioBus {
    if input.channels.len() == 1 {
        AudioBus::from_frame(stereo_panner_mono(input.channel(0), pan))
    } else {
        AudioBus::from_frame(stereo_panner_frame(input.to_frame(), pan))
    }
}

fn apply_channel_config_bus(input: AudioBus, config: ChannelConfig) -> AudioBus {
    match config.channel_count_mode {
        ChannelCountMode::Explicit => {
            if config.channel_count == 1 {
                let sample = match config.channel_interpretation {
                    ChannelInterpretation::Speakers => mix_speaker_layout(&input, 1).channel(0),
                    ChannelInterpretation::Discrete => {
                        input.channels.first().copied().unwrap_or(0.0)
                    }
                };
                return AudioBus::mono(sample);
            }
            match config.channel_interpretation {
                ChannelInterpretation::Speakers => {
                    mix_speaker_layout(&input, config.channel_count.max(1))
                }
                ChannelInterpretation::Discrete => {
                    let mut channels = vec![0.0; config.channel_count.max(1)];
                    for (index, sample) in channels.iter_mut().enumerate() {
                        *sample = input.channels.get(index).copied().unwrap_or(0.0);
                    }
                    AudioBus::from_channels(channels)
                }
            }
        }
        ChannelCountMode::ClampedMax => {
            let count = input.channels.len().min(config.channel_count.max(1));
            match config.channel_interpretation {
                ChannelInterpretation::Speakers => mix_speaker_layout(&input, count.max(1)),
                ChannelInterpretation::Discrete => {
                    let mut channels = vec![0.0; count.max(1)];
                    for (index, sample) in channels.iter_mut().enumerate() {
                        *sample = input.channels.get(index).copied().unwrap_or(0.0);
                    }
                    AudioBus::from_channels(channels)
                }
            }
        }
        ChannelCountMode::Max => input,
    }
}

fn mix_speaker_layout(input: &AudioBus, output_channels: usize) -> AudioBus {
    let output_channels = output_channels.max(1);
    let input_channels = input.channels.len();
    if input_channels == output_channels {
        return input.clone();
    }
    match (input_channels, output_channels) {
        (1, 2) => AudioBus::from_channels(vec![input.channels[0], input.channels[0]]),
        (1, 4) => AudioBus::from_channels(vec![input.channels[0], input.channels[0], 0.0, 0.0]),
        (1, 6) => AudioBus::from_channels(vec![0.0, 0.0, input.channels[0], 0.0, 0.0, 0.0]),
        (2, 1) => AudioBus::mono(0.5 * (input.channel(0) + input.channel(1))),
        (2, 4) => AudioBus::from_channels(vec![input.channel(0), input.channel(1), 0.0, 0.0]),
        (2, 6) => {
            AudioBus::from_channels(vec![input.channel(0), input.channel(1), 0.0, 0.0, 0.0, 0.0])
        }
        (4, 1) => AudioBus::mono(
            0.25 * (input.channel(0) + input.channel(1) + input.channel(2) + input.channel(3)),
        ),
        (4, 2) => AudioBus::from_channels(vec![
            input.channel(0) + 0.5 * input.channel(2),
            input.channel(1) + 0.5 * input.channel(3),
        ]),
        (4, 6) => AudioBus::from_channels(vec![
            input.channel(0),
            input.channel(1),
            0.0,
            0.0,
            input.channel(2),
            input.channel(3),
        ]),
        (6, 1) => {
            let surround_gain = 0.5;
            let front_gain = 0.5_f32.sqrt();
            AudioBus::mono(
                front_gain * (input.channel(0) + input.channel(1))
                    + input.channel(2)
                    + surround_gain * (input.channel(4) + input.channel(5)),
            )
        }
        (6, 2) => {
            let center_gain = 0.5_f32.sqrt();
            let surround_gain = 0.5;
            AudioBus::from_channels(vec![
                input.channel(0)
                    + center_gain * input.channel(2)
                    + surround_gain * input.channel(4),
                input.channel(1)
                    + center_gain * input.channel(2)
                    + surround_gain * input.channel(5),
            ])
        }
        (6, 4) => {
            let gain = 0.5_f32.sqrt();
            AudioBus::from_channels(vec![
                input.channel(0) + gain * input.channel(2),
                input.channel(1) + gain * input.channel(2),
                input.channel(4),
                input.channel(5),
            ])
        }
        _ => mix_discrete_layout(input, output_channels),
    }
}

fn mix_discrete_layout(input: &AudioBus, output_channels: usize) -> AudioBus {
    let mut channels = vec![0.0; output_channels.max(1)];
    for (index, sample) in channels.iter_mut().enumerate() {
        *sample = input.channels.get(index).copied().unwrap_or(0.0);
    }
    AudioBus::from_channels(channels)
}

fn audio_bus_has_signal(bus: &AudioBus) -> bool {
    bus.channels
        .iter()
        .any(|sample| sample.abs() > f32::EPSILON)
}

fn downmix_bus_to_mono(input: AudioBus) -> f32 {
    mix_speaker_layout(&input, 1).channel(0)
}

fn validate_panner_angle(degrees: f32) -> Result<(), GraphError> {
    if !degrees.is_finite() || !(0.0..=360.0).contains(&degrees) {
        return Err(GraphError::InvalidPannerConfig);
    }
    Ok(())
}

fn validate_positive_panner_value(value: f32) -> Result<(), GraphError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(GraphError::InvalidPannerConfig);
    }
    Ok(())
}

fn validate_non_negative_panner_value(value: f32) -> Result<(), GraphError> {
    if !value.is_finite() || value < 0.0 {
        return Err(GraphError::InvalidPannerConfig);
    }
    Ok(())
}

fn validate_audio_worklet_parameters(
    descriptors: &[AudioWorkletParameterDescriptor],
    parameter_data: &HashMap<String, f32>,
) -> bool {
    let mut names = HashSet::new();
    for descriptor in descriptors {
        if descriptor.name.is_empty()
            || !names.insert(descriptor.name.as_str())
            || !descriptor.default_value.is_finite()
            || !descriptor.min_value.is_finite()
            || !descriptor.max_value.is_finite()
            || descriptor.min_value > descriptor.default_value
            || descriptor.default_value > descriptor.max_value
        {
            return false;
        }
    }
    parameter_data.iter().all(|(name, value)| {
        value.is_finite()
            && descriptors
                .iter()
                .any(|descriptor| descriptor.name.as_str() == name.as_str())
            && descriptors
                .iter()
                .find(|descriptor| descriptor.name.as_str() == name.as_str())
                .is_none_or(|descriptor| {
                    *value >= descriptor.min_value && *value <= descriptor.max_value
                })
    })
}

fn normalize_periodic_wave_coefficients(real: &mut [f32], imag: &mut [f32]) {
    const NORMALIZATION_SAMPLES: usize = 2048;
    let harmonic_count = real.len().max(imag.len());
    let peak = (0..NORMALIZATION_SAMPLES)
        .map(|index| {
            let phase = TAU * index as f32 / NORMALIZATION_SAMPLES as f32;
            (1..harmonic_count)
                .map(|harmonic| {
                    let angle = phase * harmonic as f32;
                    let real = real.get(harmonic).copied().unwrap_or(0.0);
                    let imag = imag.get(harmonic).copied().unwrap_or(0.0);
                    real * angle.cos() + imag * angle.sin()
                })
                .sum::<f32>()
                .abs()
        })
        .fold(0.0, f32::max);
    if peak > f32::EPSILON {
        for coefficient in real.iter_mut().chain(imag.iter_mut()) {
            *coefficient /= peak;
        }
    }
}

fn source_is_active(time: f64, start_time: f64, stop_time: Option<f64>) -> bool {
    time >= start_time && stop_time.is_none_or(|stop_time| time < stop_time)
}

fn k_rate_quantum_start(time: f64, sample_dt: f64) -> f64 {
    if sample_dt.is_finite() && sample_dt > 0.0 {
        let quantum_duration = sample_dt * RENDER_QUANTUM_SIZE;
        (time / quantum_duration).floor() * quantum_duration
    } else {
        time
    }
}

fn frame_index_for_time(time: f64, quantum_start: f64, sample_dt: f64, frames: usize) -> usize {
    if !time.is_finite() || !sample_dt.is_finite() || sample_dt <= 0.0 {
        return frames;
    }
    ((time - quantum_start) / sample_dt).ceil().max(0.0) as usize
}

fn source_has_ended(time: f64, stop_time: Option<f64>) -> bool {
    stop_time.is_some_and(|stop_time| time >= stop_time)
}

fn buffer_source_has_naturally_ended(
    time: f64,
    start_time: f64,
    offset: f64,
    _duration: Option<f64>,
    looping: bool,
    buffer: &AudioBuffer,
) -> bool {
    if time < start_time || looping {
        return false;
    }
    let elapsed = time - start_time;
    offset + elapsed >= buffer.duration() as f64
}

struct AudioBufferTimeline<'a> {
    start_time: f64,
    offset: f64,
    duration: Option<f64>,
    looping: bool,
    buffer: &'a AudioBuffer,
    playback_rate: &'a ParamTimeline,
    detune: &'a ParamTimeline,
    time: f64,
}

fn audio_buffer_source_timeline_has_ended(timeline: AudioBufferTimeline<'_>) -> bool {
    if timeline.time < timeline.start_time || timeline.looping {
        return false;
    }
    let source_time = audio_buffer_source_timeline_time(
        timeline.start_time,
        timeline.offset,
        timeline.playback_rate,
        timeline.detune,
        timeline.time,
    );
    buffer_source_duration_elapsed(source_time, timeline.offset, timeline.duration)
        || buffer_source_time_out_of_bounds(source_time, timeline.buffer.duration())
}

fn audio_buffer_source_timeline_time(
    start_time: f64,
    offset: f64,
    playback_rate: &ParamTimeline,
    detune: &ParamTimeline,
    time: f64,
) -> f64 {
    if time <= start_time {
        return offset;
    }
    let mut source_time = offset;
    let mut cursor = start_time;
    let step = (1.0_f64 / 128.0).min(time - start_time).max(f64::EPSILON);
    while cursor < time {
        let dt = (time - cursor).min(step);
        let mut rate = playback_rate.value_at(cursor);
        rate *= 2.0f32.powf(detune.value_at(cursor) / 1200.0);
        if rate.is_finite() {
            source_time += dt * rate as f64;
        }
        cursor += dt;
    }
    source_time
}

fn buffer_source_time_out_of_bounds(source_time: f64, buffer_duration: f32) -> bool {
    source_time < 0.0 || source_time >= buffer_duration as f64
}

fn buffer_source_duration_elapsed(source_time: f64, offset: f64, duration: Option<f64>) -> bool {
    duration.is_some_and(|duration| {
        if source_time >= offset {
            source_time >= offset + duration
        } else {
            source_time <= offset - duration
        }
    })
}

fn effective_loop_range(loop_range: Option<(f64, f64)>, buffer_duration: f64) -> (f64, f64) {
    let (loop_start, loop_end) = loop_range.unwrap_or((0.0, 0.0));
    let loop_start = loop_start.max(0.0).min(buffer_duration);
    let loop_end = if loop_end > 0.0 {
        loop_end.min(buffer_duration)
    } else {
        buffer_duration
    };
    if loop_end > loop_start {
        (loop_start, loop_end)
    } else {
        (0.0, buffer_duration)
    }
}

fn wrap_loop_source_time(
    source_time: f64,
    loop_start: f64,
    loop_end: f64,
    playback_rate: f32,
) -> f64 {
    let loop_duration = loop_end - loop_start;
    if loop_duration <= f64::EPSILON {
        return source_time;
    }
    if source_time >= loop_end || (playback_rate < 0.0 && source_time < loop_start) {
        loop_start + (source_time - loop_start).rem_euclid(loop_duration)
    } else {
        source_time
    }
}

fn shape_sample(input: f32, curve: &[f32]) -> f32 {
    if curve.is_empty() {
        return input;
    }
    if curve.len() == 1 {
        return curve[0];
    }
    let position = ((input.clamp(-1.0, 1.0) + 1.0) * 0.5) * (curve.len() - 1) as f32;
    let left = position.floor() as usize;
    let right = position.ceil() as usize;
    if left == right {
        curve[left]
    } else {
        let amount = position - left as f32;
        curve[left] + (curve[right] - curve[left]) * amount
    }
}

fn process_waveshaper(
    input: AudioBus,
    curve: &[f32],
    oversample: Oversample,
    previous_input: &mut Option<AudioBus>,
) -> AudioBus {
    let factor = match oversample {
        Oversample::None => 1,
        Oversample::TwoX => 2,
        Oversample::FourX => 4,
    };
    let previous = previous_input.clone().unwrap_or_else(|| input.clone());
    let mut output = AudioBus::silent(input.channels.len().max(previous.channels.len()));
    for channel in 0..output.channels.len() {
        let start = previous.channels.get(channel).copied().unwrap_or(0.0);
        let end = input.channels.get(channel).copied().unwrap_or(0.0);
        output.channels[channel] = if factor == 1 {
            shape_sample(end, curve)
        } else {
            oversampled_shape_sample(start, end, curve, factor)
        };
    }
    *previous_input = Some(input);
    output
}

fn oversampled_shape_sample(start: f32, end: f32, curve: &[f32], factor: usize) -> f32 {
    let alpha = 1.0 / factor as f32;
    let mut filtered = shape_sample(start, curve);
    for step in 0..factor {
        let amount = (step + 1) as f32 / factor as f32;
        let sample = start + (end - start) * amount;
        let shaped = shape_sample(sample, curve);
        filtered += (shaped - filtered) * alpha;
    }
    filtered
}

fn read_fractional_delay(buffer: &[AudioBus], write_index: usize, delay_samples: f64) -> AudioBus {
    if buffer.is_empty() {
        return AudioBus::silent(1);
    }
    if delay_samples <= f64::EPSILON {
        return buffer[(write_index + buffer.len() - 1) % buffer.len()].clone();
    }
    let base_delay = delay_samples.floor() as usize;
    let fraction = (delay_samples - base_delay as f64) as f32;
    let first = delayed_sample(buffer, write_index, base_delay.max(1));
    if fraction <= f32::EPSILON {
        return first;
    }
    let second = delayed_sample(buffer, write_index, base_delay + 1);
    interpolate_bus(&first, &second, fraction)
}

fn process_delay_sample(
    buffer: &mut [AudioBus],
    write_index: usize,
    input: AudioBus,
    delay_samples: f64,
) -> AudioBus {
    if delay_samples <= f64::EPSILON {
        if let Some(slot) = buffer.get_mut(write_index) {
            *slot = input.clone();
        }
        return input;
    }
    let delayed = read_fractional_delay(buffer, write_index, delay_samples);
    if let Some(slot) = buffer.get_mut(write_index) {
        *slot = input;
    }
    delayed
}

fn delayed_sample(buffer: &[AudioBus], write_index: usize, delay_samples: usize) -> AudioBus {
    let index = (write_index + buffer.len() - delay_samples % buffer.len()) % buffer.len();
    buffer[index].clone()
}

fn interpolate_bus(first: &AudioBus, second: &AudioBus, amount: f32) -> AudioBus {
    let channels = first.channels.len().max(second.channels.len()).max(1);
    let mut output = AudioBus::silent(channels);
    for channel in 0..channels {
        output.channels[channel] =
            first.channel(channel) * (1.0 - amount) + second.channel(channel) * amount;
    }
    output
}

fn process_iir(
    input: AudioBus,
    feedforward: &[f32],
    feedback: &[f32],
    x_history: &mut Vec<AudioBus>,
    y_history: &mut Vec<AudioBus>,
) -> AudioBus {
    if feedforward.is_empty() {
        return input;
    }
    let channels = input.channels.len().max(1);
    x_history.insert(0, input);
    x_history.truncate(feedforward.len());

    let mut output = AudioBus::silent(channels);
    output.channels.fill(0.0);
    for (coefficient, sample) in feedforward.iter().zip(x_history.iter()) {
        for channel in 0..channels {
            output.channels[channel] += sample.channel(channel) * *coefficient;
        }
    }
    for (coefficient, sample) in feedback.iter().skip(1).zip(y_history.iter()) {
        for channel in 0..channels {
            output.channels[channel] -= sample.channel(channel) * *coefficient;
        }
    }
    let normalization = feedback.first().copied().unwrap_or(1.0);
    if normalization.abs() > f32::EPSILON {
        for channel in 0..output.channels.len() {
            output.channels[channel] /= normalization;
        }
    }
    y_history.insert(0, output.clone());
    y_history.truncate(feedback.len().saturating_sub(1));
    output
}

fn iir_frequency_response(
    feedforward: &[f32],
    feedback: &[f32],
    frequency_hz: f32,
    sample_rate: f32,
) -> (f32, f32) {
    let nyquist = (sample_rate * 0.5).max(1.0);
    if !valid_frequency_response_input(frequency_hz, nyquist) {
        return (f32::NAN, f32::NAN);
    }
    let omega = TAU * frequency_hz / sample_rate.max(1.0);
    let (numerator_re, numerator_im) = polynomial_response(feedforward, omega);
    let (denominator_re, denominator_im) = polynomial_response(feedback, omega);
    let denominator_mag_squared =
        denominator_re.mul_add(denominator_re, denominator_im * denominator_im);
    if denominator_mag_squared <= f32::EPSILON {
        return (0.0, 0.0);
    }
    let response_re =
        (numerator_re * denominator_re + numerator_im * denominator_im) / denominator_mag_squared;
    let response_im =
        (numerator_im * denominator_re - numerator_re * denominator_im) / denominator_mag_squared;
    let magnitude = response_re
        .mul_add(response_re, response_im * response_im)
        .sqrt();
    let phase = response_im.atan2(response_re);
    (magnitude, phase)
}

fn validate_iir_coefficients(feedforward: &[f32], feedback: &[f32]) -> Result<(), GraphError> {
    if feedforward.is_empty()
        || feedforward.len() > 20
        || feedback.is_empty()
        || feedback.len() > 20
        || feedback[0] == 0.0
        || feedforward.iter().all(|coefficient| *coefficient == 0.0)
        || feedforward
            .iter()
            .chain(feedback.iter())
            .any(|coefficient| !coefficient.is_finite())
        || !iir_feedback_is_stable(feedback)
    {
        return Err(GraphError::InvalidIirFilter);
    }
    Ok(())
}

fn iir_feedback_is_stable(feedback: &[f32]) -> bool {
    if feedback.len() <= 1 {
        return true;
    }
    let a0 = feedback[0] as f64;
    if a0.abs() <= f64::EPSILON {
        return false;
    }
    if feedback.len() == 2 {
        return (-feedback[1] as f64 / a0).abs() < 1.0;
    }

    let coefficients = feedback
        .iter()
        .map(|coefficient| *coefficient as f64 / a0)
        .collect::<Vec<_>>();
    polynomial_roots(&coefficients)
        .into_iter()
        .all(|root| root.magnitude() < 1.0)
}

#[derive(Debug, Clone, Copy)]
struct ComplexRoot {
    real: f64,
    imaginary: f64,
}

impl ComplexRoot {
    const ZERO: Self = Self {
        real: 0.0,
        imaginary: 0.0,
    };

    fn new(real: f64, imaginary: f64) -> Self {
        Self { real, imaginary }
    }

    fn magnitude(self) -> f64 {
        self.real.hypot(self.imaginary)
    }

    fn from_polar(radius: f64, angle: f64) -> Self {
        Self::new(radius * angle.cos(), radius * angle.sin())
    }
}

impl std::ops::Add for ComplexRoot {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.real + rhs.real, self.imaginary + rhs.imaginary)
    }
}

impl std::ops::Sub for ComplexRoot {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.real - rhs.real, self.imaginary - rhs.imaginary)
    }
}

impl std::ops::Mul for ComplexRoot {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(
            self.real * rhs.real - self.imaginary * rhs.imaginary,
            self.real * rhs.imaginary + self.imaginary * rhs.real,
        )
    }
}

impl std::ops::Div for ComplexRoot {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        let denominator = rhs.real.mul_add(rhs.real, rhs.imaginary * rhs.imaginary);
        Self::new(
            (self.real * rhs.real + self.imaginary * rhs.imaginary) / denominator,
            (self.imaginary * rhs.real - self.real * rhs.imaginary) / denominator,
        )
    }
}

fn polynomial_roots(coefficients: &[f64]) -> Vec<ComplexRoot> {
    let degree = coefficients.len().saturating_sub(1);
    if degree == 0 {
        return Vec::new();
    }
    let radius = 1.0
        + coefficients
            .iter()
            .skip(1)
            .map(|coefficient| coefficient.abs())
            .fold(0.0, f64::max);
    let mut roots = (0..degree)
        .map(|index| {
            ComplexRoot::from_polar(radius, std::f64::consts::TAU * index as f64 / degree as f64)
        })
        .collect::<Vec<_>>();

    for _ in 0..100 {
        let mut max_delta: f64 = 0.0;
        for index in 0..degree {
            let root = roots[index];
            let mut denominator = ComplexRoot::new(1.0, 0.0);
            for (other_index, other) in roots.iter().enumerate() {
                if index != other_index {
                    denominator = denominator * (root - *other);
                }
            }
            if denominator.magnitude() <= f64::EPSILON {
                denominator = ComplexRoot::new(f64::EPSILON, f64::EPSILON);
            }
            let delta = evaluate_polynomial(coefficients, root) / denominator;
            roots[index] = root - delta;
            max_delta = max_delta.max(delta.magnitude());
        }
        if max_delta < 1e-12 {
            break;
        }
    }

    roots
}

fn evaluate_polynomial(coefficients: &[f64], value: ComplexRoot) -> ComplexRoot {
    coefficients
        .iter()
        .fold(ComplexRoot::ZERO, |acc, coefficient| {
            acc * value + ComplexRoot::new(*coefficient, 0.0)
        })
}

fn valid_frequency_response_input(frequency_hz: f32, nyquist: f32) -> bool {
    frequency_hz.is_finite() && (0.0..=nyquist).contains(&frequency_hz)
}

fn polynomial_response(coefficients: &[f32], omega: f32) -> (f32, f32) {
    let mut real = 0.0;
    let mut imaginary = 0.0;
    for (index, coefficient) in coefficients.iter().enumerate() {
        let angle = omega * index as f32;
        real += coefficient * angle.cos();
        imaginary -= coefficient * angle.sin();
    }
    (real, imaginary)
}

fn process_convolver(
    input: AudioBus,
    buffer: &AudioBuffer,
    normalize: bool,
    history: &mut Vec<AudioBus>,
) -> AudioBus {
    let impulse_len = buffer.len();
    if impulse_len == 0 {
        return input;
    }
    if history.len() != impulse_len {
        *history = vec![AudioBus::silent(input.channels.len()); impulse_len];
    }
    let input_channels = input.channels.len();
    let impulse_channels = buffer.number_of_channels();
    let output_channels = if impulse_channels == 4 {
        2
    } else if input_channels == 1 && impulse_channels == 1 {
        1
    } else {
        input_channels.max(2)
    };
    history.insert(0, input);
    history.truncate(impulse_len);

    let mut output = AudioBus::silent(output_channels);
    let gain = if normalize {
        buffer.convolver_normalization_scale()
    } else {
        1.0
    };
    for (index, sample) in history.iter().enumerate() {
        let impulse = buffer.bus_at_index(index);
        if impulse_channels == 4 {
            let input_left = sample.channel(0);
            let input_right = sample.channel(1);
            output.channels[0] +=
                (input_left * impulse.channel(0) + input_right * impulse.channel(2)) * gain;
            output.channels[1] +=
                (input_left * impulse.channel(1) + input_right * impulse.channel(3)) * gain;
        } else {
            for channel in 0..output.channels.len() {
                output.channels[channel] +=
                    sample.channel(channel) * impulse.channel(channel) * gain;
            }
        }
    }
    output
}

#[derive(Debug, Clone, Copy)]
struct DynamicsCompressorParams {
    threshold_db: f32,
    knee_db: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    sample_dt: f64,
}

fn compress_bus(
    input: AudioBus,
    params: DynamicsCompressorParams,
    gain_reduction_db: &mut f32,
    pre_delay: &mut VecDeque<(AudioBus, f32)>,
) -> (AudioBus, f32) {
    let target_reduction_db = input
        .channels
        .iter()
        .map(|sample| {
            compression_gain_reduction_db(
                *sample,
                params.threshold_db,
                params.knee_db,
                params.ratio,
            )
        })
        .fold(0.0, f32::min)
        .min(0.0);
    let time_constant = if target_reduction_db < *gain_reduction_db {
        params.attack
    } else {
        params.release
    };
    let alpha = smoothing_amount(time_constant, params.sample_dt);
    *gain_reduction_db += (target_reduction_db - *gain_reduction_db) * alpha;
    let gain = 10.0f32.powf(*gain_reduction_db / 20.0);
    let delayed = dynamics_compressor_delayed_input(input, gain, params.sample_dt, pre_delay);
    (delayed, (*gain_reduction_db).min(0.0))
}

fn dynamics_compressor_delayed_input(
    input: AudioBus,
    gain: f32,
    sample_dt: f64,
    pre_delay: &mut VecDeque<(AudioBus, f32)>,
) -> AudioBus {
    let lookahead_frames = (0.006 / sample_dt).round() as usize;
    if lookahead_frames < 1 {
        pre_delay.clear();
        return input.scaled(gain);
    }
    pre_delay.push_back((input, gain));
    if pre_delay.len() <= lookahead_frames {
        return AudioBus::silent(pre_delay.front().map_or(1, |(bus, _)| bus.channels.len()));
    }
    let (delayed, delayed_gain) = pre_delay
        .pop_front()
        .expect("pre-delay contains one more frame than lookahead");
    delayed.scaled(delayed_gain)
}

fn compression_gain_reduction_db(input: f32, threshold_db: f32, knee_db: f32, ratio: f32) -> f32 {
    let magnitude = input.abs();
    if magnitude <= f32::EPSILON {
        return 0.0;
    }
    let input_db = 20.0 * magnitude.log10();
    let ratio = ratio.max(1.0);
    let knee_db = knee_db.max(0.0);
    let over_db = input_db - threshold_db;
    let gain_db = if knee_db > 0.0 && over_db > -0.5 * knee_db && over_db < 0.5 * knee_db {
        let knee_position = over_db + 0.5 * knee_db;
        (1.0 / ratio - 1.0) * knee_position * knee_position / (2.0 * knee_db)
    } else if over_db > 0.0 {
        over_db * (1.0 / ratio - 1.0)
    } else {
        0.0
    };
    gain_db.min(0.0)
}

fn smoothing_amount(time_constant: f32, sample_dt: f64) -> f32 {
    if time_constant <= f32::EPSILON || !time_constant.is_finite() {
        1.0
    } else {
        (1.0 - (-sample_dt / time_constant as f64).exp()) as f32
    }
}

fn listener_relative_coordinates(
    vector: [f32; 3],
    listener_forward: [f32; 3],
    listener_up: [f32; 3],
) -> [f32; 3] {
    let forward = normalize_vector(listener_forward).unwrap_or([0.0, 0.0, -1.0]);
    let up = normalize_vector(listener_up).unwrap_or([0.0, 1.0, 0.0]);
    let right = normalize_vector(cross_vector(forward, up)).unwrap_or([1.0, 0.0, 0.0]);
    let up = normalize_vector(cross_vector(right, forward)).unwrap_or(up);

    [
        dot_vector(vector, right),
        dot_vector(vector, up),
        dot_vector(vector, forward),
    ]
}

fn normalize_vector(vector: [f32; 3]) -> Option<[f32; 3]> {
    let len = dot_vector(vector, vector).sqrt();
    if len <= f32::EPSILON || !len.is_finite() {
        return None;
    }
    Some([vector[0] / len, vector[1] / len, vector[2] / len])
}

fn dot_vector(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross_vector(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

#[derive(Debug, Clone, Copy)]
struct PannerSpatialParams {
    position: [f32; 3],
    orientation: [f32; 3],
    distance_model: DistanceModel,
    ref_distance: f32,
    max_distance: f32,
    rolloff_factor: f32,
    cone_inner_angle: f32,
    cone_outer_angle: f32,
    cone_outer_gain: f32,
}

fn pan_position(input: &AudioBus, params: PannerSpatialParams) -> AudioBus {
    let [x, y, z] = params.position;
    let distance = (x * x + y * y + z * z).sqrt();
    let ref_distance = params.ref_distance.max(f32::MIN_POSITIVE);
    let max_distance = params.max_distance.max(ref_distance);
    let distance = distance.max(ref_distance);
    let attenuation = match params.distance_model {
        DistanceModel::Linear => {
            let span = (max_distance - ref_distance).max(f32::MIN_POSITIVE);
            (1.0 - params.rolloff_factor * (distance - ref_distance) / span).clamp(0.0, 1.0)
        }
        DistanceModel::Inverse => {
            ref_distance / (ref_distance + params.rolloff_factor * (distance - ref_distance))
        }
        DistanceModel::Exponential => (distance / ref_distance).powf(-params.rolloff_factor),
    };
    let cone_gain = cone_gain(params);
    let pan = if x.abs() <= f32::EPSILON {
        0.0
    } else {
        (x / (1.0 + x.abs())).clamp(-1.0, 1.0)
    };
    stereo_panner_bus(&input.scaled(attenuation * cone_gain), pan)
}

fn cone_gain(params: PannerSpatialParams) -> f32 {
    if params.cone_inner_angle >= 360.0 && params.cone_outer_angle >= 360.0 {
        return 1.0;
    }
    let [x, y, z] = params.position;
    let [orientation_x, orientation_y, orientation_z] = params.orientation;
    let source_len = (x * x + y * y + z * z).sqrt();
    let orientation_len = (orientation_x * orientation_x
        + orientation_y * orientation_y
        + orientation_z * orientation_z)
        .sqrt();
    if source_len <= f32::EPSILON || orientation_len <= f32::EPSILON {
        return 1.0;
    }
    let dot = (-(x * orientation_x + y * orientation_y + z * orientation_z))
        / (source_len * orientation_len);
    let angle = dot.clamp(-1.0, 1.0).acos().to_degrees();
    let inner = params.cone_inner_angle.min(params.cone_outer_angle) * 0.5;
    let outer = params.cone_outer_angle.max(params.cone_inner_angle) * 0.5;
    if angle <= inner {
        1.0
    } else if angle >= outer || (outer - inner).abs() <= f32::EPSILON {
        params.cone_outer_gain
    } else {
        let amount = (angle - inner) / (outer - inner);
        1.0 + (params.cone_outer_gain - 1.0) * amount
    }
}
