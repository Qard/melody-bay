#[derive(Debug, Clone, PartialEq)]
struct NoteEvent {
    id: EventId,
    start: SequenceTime,
    duration: f64,
    frequency: f32,
    base_frequency: f32,
    velocity: f32,
}

impl NoteEvent {
    fn active_at(&self, time: f64) -> bool {
        time >= self.start.as_seconds() && time < self.start.as_seconds() + self.duration
    }

    fn gate_gain(&self, local_time: f64) -> f32 {
        const RELEASE_SECONDS: f64 = 0.018;

        if self.duration <= f64::EPSILON {
            return 0.0;
        }
        let remaining = self.duration - local_time;
        let release = RELEASE_SECONDS.min(self.duration * 0.5);
        if release <= f64::EPSILON || remaining >= release {
            return 1.0;
        }
        let x = (remaining / release).clamp(0.0, 1.0) as f32;
        x * x * (3.0 - 2.0 * x)
    }

    fn pitch_ratio(&self) -> f32 {
        if self.base_frequency.is_finite() && self.base_frequency > 0.0 {
            self.frequency / self.base_frequency
        } else {
            1.0
        }
    }
}

fn timeline_value_for_render_cached(
    timeline: &ParamTimeline,
    local_time: f64,
    sequence_time: f64,
    sample_dt: f64,
    runtime: &mut ParamTimelineRuntime,
) -> f32 {
    let timeline_time = match timeline.time_domain {
        ParamTimeDomain::Local => local_time,
        ParamTimeDomain::Global => sequence_time,
    };
    let automation_time = match timeline.automation_rate() {
        AutomationRate::ARate => timeline_time,
        AutomationRate::KRate if sample_dt.is_finite() && sample_dt > 0.0 => {
            let quantum_duration = sample_dt * RENDER_QUANTUM_SIZE;
            (timeline_time / quantum_duration).floor() * quantum_duration
        }
        AutomationRate::KRate => timeline_time,
    };
    timeline.value_at_monotonic(automation_time, runtime)
}

#[derive(Debug, Clone, Copy)]
struct SampleVoiceFrame {
    frame: Frame,
    mono: bool,
}

fn pan_sample_voice_frame(sample: SampleVoiceFrame, gain: f32, pan: f32) -> Frame {
    if sample.mono {
        stereo_panner_mono(sample.frame.left * gain, pan)
    } else {
        stereo_panner_frame(sample.frame * gain, pan)
    }
}

fn sample_voice_frame_at(buffer: &AudioBuffer, time: f64) -> SampleVoiceFrame {
    if !time.is_finite() || time <= 0.0 {
        return sample_voice_frame_at_index(buffer, 0);
    }
    let position = time * buffer.sample_rate as f64;
    let index = position.floor() as usize;
    let fraction = (position - index as f64) as f32;
    let first = sample_voice_frame_at_index(buffer, index);
    if fraction <= f32::EPSILON {
        return first;
    }
    let second = sample_voice_frame_at_index(buffer, index.saturating_add(1));
    interpolate_sample_voice_frame(first, second, fraction)
}

fn sample_voice_frame_at_looping(
    buffer: &AudioBuffer,
    time: f64,
    loop_start: f64,
    loop_end: f64,
) -> SampleVoiceFrame {
    if !time.is_finite() || time <= 0.0 {
        return sample_voice_frame_at_index(buffer, 0);
    }
    let position = time * buffer.sample_rate as f64;
    let index = position.floor().max(0.0) as usize;
    let fraction = (position - index as f64) as f32;
    let first = sample_voice_frame_at_index(buffer, index);
    if fraction <= f32::EPSILON {
        return first;
    }
    let loop_start_frame = (loop_start * buffer.sample_rate as f64).round().max(0.0) as usize;
    let loop_end_frame = (loop_end * buffer.sample_rate as f64)
        .round()
        .max(loop_start_frame as f64 + 1.0) as usize;
    let next = index.saturating_add(1);
    let next = if next >= loop_end_frame {
        loop_start_frame
    } else {
        next
    };
    let second = sample_voice_frame_at_index(buffer, next);
    interpolate_sample_voice_frame(first, second, fraction)
}

fn sample_voice_frame_between(
    buffer: &AudioBuffer,
    start_time: f64,
    end_time: f64,
    loop_range: Option<(f64, f64)>,
) -> SampleVoiceFrame {
    if !start_time.is_finite() || !end_time.is_finite() || end_time <= start_time {
        return if let Some((loop_start, loop_end)) = loop_range {
            sample_voice_frame_at_looping(buffer, start_time, loop_start, loop_end)
        } else {
            sample_voice_frame_at(buffer, start_time)
        };
    }
    let source_frames = (end_time - start_time).abs() * buffer.sample_rate as f64;
    if source_frames <= 1.0 {
        return if let Some((loop_start, loop_end)) = loop_range {
            sample_voice_frame_at_looping(buffer, start_time, loop_start, loop_end)
        } else {
            sample_voice_frame_at(buffer, start_time)
        };
    }

    let samples = (source_frames.ceil() as usize).clamp(2, 32);
    let mono = buffer.number_of_channels() == 1;
    let mut sum = Frame::ZERO;
    for sample_index in 0..samples {
        let amount = (sample_index as f64 + 0.5) / samples as f64;
        let time = start_time + (end_time - start_time) * amount;
        let frame = if let Some((loop_start, loop_end)) = loop_range {
            let time = wrap_loop_source_time(time, loop_start, loop_end, 1.0);
            sample_voice_frame_at_looping(buffer, time, loop_start, loop_end)
        } else {
            sample_voice_frame_at(buffer, time)
        };
        sum += frame.frame;
    }
    SampleVoiceFrame {
        frame: sum * (1.0 / samples as f32),
        mono,
    }
}

fn sample_voice_frame_at_index(buffer: &AudioBuffer, index: usize) -> SampleVoiceFrame {
    let channels = buffer.number_of_channels();
    let left = sample_voice_channel_at_index(buffer, 0, index);
    if channels <= 1 {
        SampleVoiceFrame {
            frame: Frame::new(left, left),
            mono: true,
        }
    } else {
        SampleVoiceFrame {
            frame: Frame::new(left, sample_voice_channel_at_index(buffer, 1, index)),
            mono: false,
        }
    }
}

fn sample_voice_channel_at_index(buffer: &AudioBuffer, channel: usize, index: usize) -> f32 {
    buffer
        .channels
        .get(channel)
        .and_then(|samples| samples.get(index))
        .copied()
        .unwrap_or(0.0)
}

fn interpolate_sample_voice_frame(
    first: SampleVoiceFrame,
    second: SampleVoiceFrame,
    amount: f32,
) -> SampleVoiceFrame {
    SampleVoiceFrame {
        frame: first.frame + (second.frame - first.frame) * amount,
        mono: first.mono && second.mono,
    }
}

#[derive(Debug)]
struct HandleState {
    gain_bits: AtomicU32,
    stopped: AtomicBool,
}

impl Default for HandleState {
    fn default() -> Self {
        Self {
            gain_bits: AtomicU32::new(1.0f32.to_bits()),
            stopped: AtomicBool::new(false),
        }
    }
}

#[derive(Debug)]
#[derive(Default)]
struct VoiceRuntime {
    graph_nodes: Vec<NodeRuntime>,
    sample_source_time: Option<f64>,
    sample_voice_params: SampleVoiceParamRuntime,
}


