fn handle_tracker_effects(
    format: ImportedFormat,
    sequence: &mut IndexedSequence,
    warnings: &mut Vec<ImportWarning>,
    timing: &mut TrackerTiming,
    row: u64,
    cell: TrackerCell,
) {
    match cell.effect {
        0x0f if cell.effect_param > 0 => {
            if cell.effect_param <= 32 {
                timing.speed = u32::from(cell.effect_param).max(1);
            } else {
                timing.bpm = u32::from(cell.effect_param).max(1);
            }
            let bpm = match format {
                ImportedFormat::Mod | ImportedFormat::Xm => f64::from(timing.bpm),
                ImportedFormat::Midi => timing.rows_per_minute(),
            };
            sequence.tempo_at(row, bpm);
        }
        0x00..=0x0d | 0x0f => {}
        0x0e if matches!(cell.effect_param >> 4, 0x09 | 0x0c | 0x0d) => {}
        0x00..=0x0e => {
            if cell.effect != 0 || cell.effect_param != 0 {
                warnings.push(ImportWarning::unsupported_effect(
                    format,
                    format!("{:x}{:02x}", cell.effect, cell.effect_param),
                ));
            }
        }
        _ => warnings.push(ImportWarning::unsupported_effect(
            format,
            format!("{:x}{:02x}", cell.effect, cell.effect_param),
        )),
    }
}

fn tracker_pattern_break_row(param: u8) -> usize {
    let tens = usize::from((param >> 4).min(9));
    let ones = usize::from((param & 0x0f).min(9));
    tens * 10 + ones
}

fn apply_tracker_cell_automation(
    track: &mut IndexedTrack,
    row: u64,
    cell: TrackerCell,
    _default_volume: f32,
    _mix_gain: f32,
    timing: TrackerTiming,
    playback_rate_correction: f32,
) {
    if (playback_rate_correction - 1.0).abs() > 0.0001 {
        track.automation.push(IndexedAutomationEvent {
            index: row,
            target: "source.playbackRate".to_owned(),
            shape: IndexedAutomationShape::SetValue {
                value: playback_rate_correction,
            },
        });
    }
    if cell.effect == 0x00 && cell.effect_param != 0 {
        let x = cell.effect_param >> 4;
        let y = cell.effect_param & 0x0f;
        let speed = timing.speed.max(1);
        let values = (0..speed)
            .map(|tick| match tick % 3 {
                1 => x,
                2 => y,
                _ => 0,
            })
            .map(|semitones| 2.0f32.powf(f32::from(semitones) / 12.0))
            .map(|rate| rate * playback_rate_correction)
            .collect::<Vec<_>>();
        track.automation.push(IndexedAutomationEvent {
            index: row,
            target: "source.playbackRate".to_owned(),
            shape: IndexedAutomationShape::ValueCurve {
                values,
                duration_indices: u64::from(speed),
            },
        });
        track.automation.push(IndexedAutomationEvent {
            index: row.saturating_add(u64::from(speed)),
            target: "source.playbackRate".to_owned(),
            shape: IndexedAutomationShape::SetValue {
                value: playback_rate_correction,
            },
        });
    }
    if cell.effect == 0x08 { track.automation.push(IndexedAutomationEvent {
        index: row,
        target: "pan".to_owned(),
        shape: IndexedAutomationShape::SetValue {
            value: f32::from(cell.effect_param) / 127.5 - 1.0,
        },
    }) }
    if let Some(pan) = cell.pan {
        track.automation.push(IndexedAutomationEvent {
            index: row,
            target: "pan".to_owned(),
            shape: IndexedAutomationShape::SetValue { value: pan },
        });
    }
}

fn apply_mod_pitch_effects(
    tracks: &mut BTreeMap<(usize, usize, usize, usize), IndexedTrack>,
    active_notes: &BTreeMap<usize, ActiveTrackerNote>,
    channel_periods: &mut [Option<f32>],
    channel: usize,
    row: u64,
    cell: TrackerCell,
    timing: TrackerTiming,
) {
    let Some(active) = active_notes.get(&channel) else {
        return;
    };
    let Some(track) = tracks.get_mut(&(
        channel,
        active.instrument,
        active.sample_slot,
        active.sample_offset,
    )) else {
        return;
    };
    let Some(base_period) = active.period.filter(|period| *period > 0.0) else {
        return;
    };
    let current_period = channel_periods
        .get(channel)
        .copied()
        .flatten()
        .unwrap_or(base_period);
    let speed = timing.speed.max(1);
    let mut period = current_period;
    let values = match cell.effect {
        0x01 if cell.effect_param > 0 => {
            let slide = f32::from(cell.effect_param);
            Some(
                (0..speed)
                    .map(|tick| {
                        if tick > 0 {
                            period = (period - slide).max(1.0);
                        }
                        active.playback_rate_correction * base_period / period
                    })
                    .collect::<Vec<_>>(),
            )
        }
        0x02 if cell.effect_param > 0 => {
            let slide = f32::from(cell.effect_param);
            Some(
                (0..speed)
                    .map(|tick| {
                        if tick > 0 {
                            period += slide;
                        }
                        active.playback_rate_correction * base_period / period
                    })
                    .collect::<Vec<_>>(),
            )
        }
        0x03 | 0x05 if cell.period.is_some() && cell.effect_param > 0 => {
            let target = f32::from(cell.period.unwrap()).max(1.0);
            let slide = f32::from(cell.effect_param);
            Some(
                (0..speed)
                    .map(|tick| {
                        if tick > 0 {
                            period = slide_period_toward(period, target, slide);
                        }
                        active.playback_rate_correction * base_period / period
                    })
                    .collect::<Vec<_>>(),
            )
        }
        0x04 | 0x06 if cell.effect_param > 0 => {
            let rate = f32::from(cell.effect_param >> 4).max(1.0);
            let depth = f32::from(cell.effect_param & 0x0f) / 64.0;
            Some(
                (0..speed)
                    .map(|tick| {
                        let phase = (tick as f32 * rate / 32.0) * std::f32::consts::TAU;
                        active.playback_rate_correction * 2.0f32.powf(phase.sin() * depth / 12.0)
                    })
                    .collect::<Vec<_>>(),
            )
        }
        _ => None,
    };
    let Some(values) = values else {
        return;
    };
    if matches!(cell.effect, 0x01 | 0x02 | 0x03 | 0x05)
        && let Some(slot) = channel_periods.get_mut(channel)
    {
        *slot = Some(period);
    }
    track.automation.push(IndexedAutomationEvent {
        index: row,
        target: "source.playbackRate".to_owned(),
        shape: IndexedAutomationShape::ValueCurve {
            values,
            duration_indices: u64::from(speed),
        },
    });
}

fn slide_period_toward(current: f32, target: f32, slide: f32) -> f32 {
    if current > target {
        (current - slide).max(target)
    } else if current < target {
        (current + slide).min(target)
    } else {
        current
    }
}

fn apply_mod_retrigger_effect(
    tracks: &mut BTreeMap<(usize, usize, usize, usize), IndexedTrack>,
    active_notes: &mut BTreeMap<usize, ActiveTrackerNote>,
    channel: usize,
    row_tick: u64,
    cell: TrackerCell,
    timing: TrackerTiming,
) {
    let Some(interval) = mod_retrigger_ticks(cell) else {
        return;
    };
    if interval == 0 {
        return;
    }
    let speed = timing.speed.max(1);
    let mut retrigger_tick = u32::from(interval);
    while retrigger_tick < speed {
        let Some(active) = active_notes.get_mut(&channel) else {
            return;
        };
        let tick = row_tick.saturating_add(u64::from(retrigger_tick));
        if tick > active.start_index
            && let Some(track) = tracks.get_mut(&(
                channel,
                active.instrument,
                active.sample_slot,
                active.sample_offset,
            ))
        {
            track.notes.push(IndexedNoteEvent {
                start_index: active.start_index,
                duration_indices: tick.saturating_sub(active.start_index).max(1),
                note: Note::from_midi(active.note),
                velocity: Velocity::new(active.velocity),
            });
            active.start_index = tick;
        }
        retrigger_tick = retrigger_tick.saturating_add(u32::from(interval));
    }
}

fn mod_note_delay_ticks(cell: TrackerCell, timing: TrackerTiming) -> u8 {
    match mod_extended_effect(cell) {
        Some((0x0d, ticks)) => ticks.min(timing.speed.saturating_sub(1) as u8),
        _ => 0,
    }
}

fn mod_retrigger_ticks(cell: TrackerCell) -> Option<u8> {
    match mod_extended_effect(cell) {
        Some((0x09, ticks)) if ticks > 0 => Some(ticks),
        _ => None,
    }
}

fn mod_extended_effect(cell: TrackerCell) -> Option<(u8, u8)> {
    (cell.effect == 0x0e).then_some((cell.effect_param >> 4, cell.effect_param & 0x0f))
}

fn mod_playback_rate_correction(note: Option<u8>, period: Option<u16>) -> f32 {
    let (Some(note), Some(period)) = (note, period) else {
        return 1.0;
    };
    if period == 0 {
        return 1.0;
    }
    let exact_rate = mod_period_frequency(period) / 8_363.0;
    let midi_rate = Note::from_midi(note).frequency() / Note::from_midi(60).frequency();
    if !exact_rate.is_finite() || !midi_rate.is_finite() || midi_rate <= 0.0 {
        1.0
    } else {
        exact_rate / midi_rate
    }
}

fn mod_period_frequency(period: u16) -> f32 {
    const PAL_CLOCK_HZ: f32 = 7_093_789.2;
    PAL_CLOCK_HZ / (f32::from(period) * 2.0)
}

#[derive(Debug, Clone, Copy)]
struct TrackerVolumeContext {
    channel: usize,
    row: u64,
    cell: TrackerCell,
    timing: TrackerTiming,
    mix_gain: f32,
}

fn apply_tracker_channel_volume_effects(
    tracks: &mut BTreeMap<(usize, usize, usize, usize), IndexedTrack>,
    active_notes: &BTreeMap<usize, ActiveTrackerNote>,
    channel_volumes: &mut [f32],
    context: TrackerVolumeContext,
) {
    let Some(volume) = channel_volumes.get_mut(context.channel) else {
        return;
    };
    let speed = context.timing.speed.max(1);
    let mix_gain = context.mix_gain.clamp(0.0, 1.0);
    let automation_shape = if context.cell.effect == 0x0c {
        *volume = f32::from(context.cell.effect_param.min(64)) / 64.0;
        Some(IndexedAutomationShape::SetValue {
            value: *volume * mix_gain,
        })
    } else if context.cell.effect == 0x0a && context.cell.effect_param != 0 {
        let up = context.cell.effect_param >> 4;
        let down = context.cell.effect_param & 0x0f;
        let slide = if up > 0 {
            f32::from(up) / 64.0
        } else {
            -f32::from(down) / 64.0
        };
        let mut tick_volume = *volume;
        let values = (0..speed)
            .map(|tick| {
                if tick > 0 {
                    tick_volume = (tick_volume + slide).clamp(0.0, 1.0);
                }
                tick_volume * mix_gain
            })
            .collect::<Vec<_>>();
        *volume = tick_volume;
        Some(IndexedAutomationShape::ValueCurve {
            values,
            duration_indices: u64::from(speed),
        })
    } else {
        None
    };
    let Some(shape) = automation_shape else {
        return;
    };
    let Some(active) = active_notes.get(&context.channel) else {
        return;
    };
    if let Some(track) = tracks.get_mut(&(
        context.channel,
        active.instrument,
        active.sample_slot,
        active.sample_offset,
    )) {
        track.automation.push(IndexedAutomationEvent {
            index: context.row,
            target: "channel.gain".to_owned(),
            shape,
        });
    }
}

fn close_tracker_channel_note(
    tracks: &mut BTreeMap<(usize, usize, usize, usize), IndexedTrack>,
    active_notes: &mut BTreeMap<usize, ActiveTrackerNote>,
    channel: usize,
    end_index: u64,
) {
    if let Some(active) = active_notes.remove(&channel)
        && let Some(track) = tracks.get_mut(&(
            channel,
            active.instrument,
            active.sample_slot,
            active.sample_offset,
        )) {
            track.notes.push(IndexedNoteEvent {
                start_index: active.start_index,
                duration_indices: end_index.saturating_sub(active.start_index).max(1),
                note: Note::from_midi(active.note),
                velocity: Velocity::new(active.velocity),
            });
        }
}

fn close_all_tracker_notes(
    tracks: &mut BTreeMap<(usize, usize, usize, usize), IndexedTrack>,
    active_notes: &mut BTreeMap<usize, ActiveTrackerNote>,
    end_index: u64,
) {
    let channels = active_notes.keys().copied().collect::<Vec<_>>();
    for channel in channels {
        close_tracker_channel_note(tracks, active_notes, channel, end_index);
    }
}

fn tracker_instrument(
    sample: &TrackerSample,
    pan_override: Option<f32>,
    mix_gain: f32,
    sample_offset: usize,
) -> Instrument {
    let sample_data = tracker_sample_data_from_offset(sample, sample_offset);
    let buffer = if sample_data.is_empty() {
        AudioBuffer::from_channels(8_363, 1, [vec![0.0]])
    } else {
        AudioBuffer::from_channels(8_363, sample_data.len(), [sample_data])
    };
    let root = Note::from_midi((60i16 - i16::from(sample.relative_note)).clamp(0, 127) as u8);
    let volume = sample.volume * mix_gain.clamp(0.0, 1.0);
    let mut instrument = Instrument::sample(buffer, root)
        .volume(volume)
        .pan(pan_override.unwrap_or(sample.pan))
        .finetune_cents(-sample.finetune_cents);
    if sample_offset == 0
        && sample.loop_len > 0
        && sample.loop_start < sample.data.len()
        && sample.loop_start.saturating_add(sample.loop_len) <= sample.data.len()
    {
        instrument = instrument.loop_range(
            sample.loop_start as f64 / 8_363.0,
            sample.loop_start.saturating_add(sample.loop_len) as f64 / 8_363.0,
        );
    }
    if let Some(envelope) = &sample.envelope {
        instrument = instrument.envelope(envelope.clone());
    }
    instrument
}

fn tracker_sample_offset_frames(cell: TrackerCell, sample: &TrackerSample) -> usize {
    if cell.effect != 0x09 || cell.effect_param == 0 {
        return 0;
    }
    let byte_offset = usize::from(cell.effect_param) * 256;
    let frame_offset = if sample.is_16_bit {
        byte_offset / 2
    } else {
        byte_offset
    };
    frame_offset.min(sample.data.len())
}

fn tracker_sample_data_from_offset(sample: &TrackerSample, sample_offset: usize) -> Vec<f32> {
    if sample_offset == 0 {
        sample.data.clone()
    } else if sample_offset < sample.data.len() {
        sample.data[sample_offset..].to_vec()
    } else {
        Vec::new()
    }
}

fn expand_ping_pong_loop(sample: &mut TrackerSample) {
    if !sample.ping_pong_loop || sample.loop_len <= 2 {
        return;
    }
    let loop_end = sample.loop_start.saturating_add(sample.loop_len);
    if sample.loop_start >= sample.data.len() || loop_end > sample.data.len() {
        return;
    }
    let reverse = sample.data[sample.loop_start + 1..loop_end - 1]
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    sample.data.truncate(loop_end);
    sample.data.extend(reverse);
    sample.loop_len = sample.data.len().saturating_sub(sample.loop_start);
    sample.length = sample.data.len();
    sample.ping_pong_loop = false;
}

fn tracker_mix_gain(channels: usize) -> f32 {
    if channels == 0 {
        1.0
    } else {
        (1.6 / channels as f32).min(1.0)
    }
}

fn mod_channel_pan(channel: usize) -> f32 {
    match channel % 4 {
        0 | 3 => -1.0,
        _ => 1.0,
    }
}

fn xm_channel_pan(channel: usize, sample: &TrackerSample) -> Option<f32> {
    if sample.pan.abs() <= 0.01 {
        Some(mod_channel_pan(channel))
    } else {
        None
    }
}

fn xm_instrument_sample(
    instrument: &XmInstrument,
    midi_note: u8,
) -> Option<(usize, &TrackerSample)> {
    if instrument.samples.is_empty() {
        return None;
    }
    let key = usize::from(midi_note.saturating_sub(12)).min(95);
    let sample_index = instrument.keymap[key].min(instrument.samples.len().saturating_sub(1));
    instrument
        .samples
        .get(sample_index)
        .map(|sample| (sample_index, sample))
}

fn mod_period_to_midi(period: u16) -> Option<u8> {
    const PERIODS: [(u16, u8); 48] = [
        (1712, 36),
        (1616, 37),
        (1524, 38),
        (1440, 39),
        (1356, 40),
        (1280, 41),
        (1208, 42),
        (1140, 43),
        (1076, 44),
        (1016, 45),
        (960, 46),
        (906, 47),
        (856, 48),
        (808, 49),
        (762, 50),
        (720, 51),
        (678, 52),
        (640, 53),
        (604, 54),
        (570, 55),
        (538, 56),
        (508, 57),
        (480, 58),
        (453, 59),
        (428, 60),
        (404, 61),
        (381, 62),
        (360, 63),
        (339, 64),
        (320, 65),
        (302, 66),
        (285, 67),
        (269, 68),
        (254, 69),
        (240, 70),
        (226, 71),
        (214, 72),
        (202, 73),
        (190, 74),
        (180, 75),
        (170, 76),
        (160, 77),
        (151, 78),
        (143, 79),
        (135, 80),
        (127, 81),
        (120, 82),
        (113, 83),
    ];
    if period == 0 {
        return None;
    }
    PERIODS
        .iter()
        .min_by_key(|(candidate, _)| candidate.abs_diff(period))
        .map(|(_, note)| *note)
}

fn xm_note_to_midi(note: u8) -> Option<u8> {
    match note {
        1..=96 => Some(note.saturating_add(11)),
        _ => None,
    }
}

fn xm_volume_to_velocity(volume: u8) -> Option<f32> {
    match volume {
        0x10..=0x50 => Some(f32::from(volume - 0x10) / 64.0),
        _ => None,
    }
}

fn xm_volume_to_pan(volume: u8) -> Option<f32> {
    match volume {
        0xc0..=0xcf => Some(f32::from(volume - 0xc0) / 7.5 - 1.0),
        _ => None,
    }
}

fn trim_c_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use fmt::Write as _;
        let _ = write!(&mut text, "{byte:02x}");
    }
    text
}
