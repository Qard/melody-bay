fn import_mod_impl(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
    if bytes.len() < 600 {
        return Err(ImportError::InvalidFormat("truncated MOD header"));
    }
    let (sample_count, channels, order_offset, signature_len) = if bytes.len() >= 1_084 {
        match &bytes[1080..1084] {
            b"M.K." | b"M!K!" | b"4CHN" | b"FLT4" => (31usize, 4usize, 950usize, 4usize),
            tag if tag[1..] == *b"CHN" && tag[0].is_ascii_digit() => {
                (31usize, usize::from(tag[0] - b'0'), 950usize, 4usize)
            }
            tag if tag[..2].iter().all(u8::is_ascii_digit) && tag[2..] == *b"CH" => {
                let channels = usize::from(tag[0] - b'0') * 10 + usize::from(tag[1] - b'0');
                (31usize, channels.max(1), 950usize, 4usize)
            }
            _ => (15usize, 4usize, 470usize, 0usize),
        }
    } else {
        (15usize, 4usize, 470usize, 0usize)
    };
    let title = trim_c_string(&bytes[..20]);
    let mut samples = Vec::with_capacity(sample_count);
    let mut cursor = 20usize;
    for _ in 0..sample_count {
        let name = trim_c_string(&bytes[cursor..cursor + 22]);
        let length = usize::from(be_u16(bytes, cursor + 22)?) * 2;
        let finetune = bytes[cursor + 24] & 0x0f;
        let finetune = if finetune >= 8 {
            i8::try_from(finetune).unwrap_or(0) - 16
        } else {
            i8::try_from(finetune).unwrap_or(0)
        };
        let volume = f32::from(bytes[cursor + 25].min(64)) / 64.0;
        let loop_start = usize::from(be_u16(bytes, cursor + 26)?) * 2;
        let raw_loop_len = usize::from(be_u16(bytes, cursor + 28)?) * 2;
        let loop_len = if raw_loop_len > 2 { raw_loop_len } else { 0 };
        samples.push(TrackerSample {
            _name: name,
            length,
            is_16_bit: false,
            volume,
            finetune_cents: f32::from(finetune) * 100.0 / 8.0,
            loop_start,
            loop_len,
            ping_pong_loop: false,
            data: Vec::new(),
            relative_note: 0,
            pan: 0.0,
            envelope: None,
        });
        cursor += 30;
    }
    let song_len = usize::from(bytes[order_offset]).clamp(1, 128);
    let orders_start = order_offset + 2;
    let orders_end = orders_start + 128;
    if orders_end > bytes.len() {
        return Err(ImportError::InvalidFormat("truncated MOD order table"));
    }
    let orders = &bytes[orders_start..orders_end];
    let pattern_count = orders[..song_len]
        .iter()
        .copied()
        .max()
        .map_or(0usize, |max_order| usize::from(max_order) + 1);
    let pattern_bytes = 64usize
        .checked_mul(channels)
        .and_then(|value| value.checked_mul(4))
        .ok_or(ImportError::InvalidFormat("MOD pattern size overflow"))?;
    let patterns_start = orders_end + signature_len;
    let samples_start = patterns_start
        .checked_add(pattern_count.saturating_mul(pattern_bytes))
        .ok_or(ImportError::InvalidFormat("MOD pattern data overflow"))?;
    if samples_start > bytes.len() {
        return Err(ImportError::InvalidFormat("truncated MOD pattern data"));
    }
    let mut sample_cursor = samples_start;
    for sample in &mut samples {
        let end = sample_cursor
            .checked_add(sample.length)
            .filter(|end| *end <= bytes.len())
            .ok_or(ImportError::MalformedSampleData(
                "truncated MOD sample data",
            ))?;
        sample.data = bytes[sample_cursor..end]
            .iter()
            .map(|byte| i8::from_ne_bytes([*byte]) as f32 / 128.0)
            .collect();
        sample_cursor = end;
    }

    let mut timing = TrackerTiming { speed: 6, bpm: 125 };
    let mut sequence = IndexedSequence::new(24).title(title);
    sequence.tempo_at(0, f64::from(timing.bpm));
    let mut track_map = BTreeMap::<(usize, usize, usize, usize), IndexedTrack>::new();
    let mut active_notes = BTreeMap::<usize, ActiveTrackerNote>::new();
    let mut channel_instruments = vec![0usize; channels];
    let mut channel_volumes = vec![1.0f32; channels];
    let mut channel_periods = vec![None::<f32>; channels];
    let mut warnings = Vec::new();
    let mut order_index = 0usize;
    let mut row_start = 0usize;
    let mut output_tick = 0u64;
    let mut flow_guard = 0usize;
    let mix_gain = tracker_mix_gain(channels);
    while order_index < song_len && flow_guard < song_len.saturating_mul(256).max(1) {
        flow_guard += 1;
        let pattern_index = usize::from(orders[order_index]);
        let pattern_offset = patterns_start + pattern_index * pattern_bytes;
        let mut next_order = order_index + 1;
        let mut next_row_start = 0usize;
        let mut flow_changed = false;
        for row in row_start..64usize {
            let global_tick = output_tick;
            for channel in 0..channels {
                let cell_offset = pattern_offset + (row * channels + channel) * 4;
                let cell = parse_mod_cell(&bytes[cell_offset..cell_offset + 4]);
                if cell.effect == 0x0b {
                    next_order = usize::from(cell.effect_param);
                    next_row_start = 0;
                    flow_changed = true;
                } else if cell.effect == 0x0d {
                    next_order = order_index + 1;
                    next_row_start = tracker_pattern_break_row(cell.effect_param);
                    flow_changed = true;
                }
                handle_tracker_effects(
                    ImportedFormat::Mod,
                    &mut sequence,
                    &mut warnings,
                    &mut timing,
                    global_tick,
                    cell,
                );
                if cell.key_off {
                    close_tracker_channel_note(
                        &mut track_map,
                        &mut active_notes,
                        channel,
                        global_tick,
                    );
                }
                if cell.instrument > 0 {
                    channel_instruments[channel] = cell.instrument;
                }
                let tone_portamento_note =
                    matches!(cell.effect, 0x03 | 0x05) && cell.note.is_some();
                let Some(note) = cell.note else {
                    apply_tracker_channel_volume_effects(
                        &mut track_map,
                        &active_notes,
                        &mut channel_volumes,
                        TrackerVolumeContext {
                            channel,
                            row: global_tick,
                            cell,
                            timing,
                            mix_gain: tracker_mix_gain(channels),
                        },
                    );
                    apply_mod_pitch_effects(
                        &mut track_map,
                        &active_notes,
                        &mut channel_periods,
                        channel,
                        global_tick,
                        cell,
                        timing,
                    );
                    apply_mod_retrigger_effect(
                        &mut track_map,
                        &mut active_notes,
                        channel,
                        global_tick,
                        cell,
                        timing,
                    );
                    continue;
                };
                if tone_portamento_note && active_notes.contains_key(&channel) {
                    apply_mod_pitch_effects(
                        &mut track_map,
                        &active_notes,
                        &mut channel_periods,
                        channel,
                        global_tick,
                        cell,
                        timing,
                    );
                    apply_tracker_channel_volume_effects(
                        &mut track_map,
                        &active_notes,
                        &mut channel_volumes,
                        TrackerVolumeContext {
                            channel,
                            row: global_tick,
                            cell,
                            timing,
                            mix_gain,
                        },
                    );
                    apply_mod_retrigger_effect(
                        &mut track_map,
                        &mut active_notes,
                        channel,
                        global_tick,
                        cell,
                        timing,
                    );
                    continue;
                }
                let note_start_tick =
                    global_tick.saturating_add(u64::from(mod_note_delay_ticks(cell, timing)));
                close_tracker_channel_note(
                    &mut track_map,
                    &mut active_notes,
                    channel,
                    note_start_tick,
                );
                let instrument = if cell.instrument > 0 {
                    cell.instrument
                } else {
                    channel_instruments[channel]
                };
                if instrument == 0 || instrument > samples.len() {
                    warnings.push(ImportWarning::new("MOD note without valid instrument"));
                    continue;
                }
                let sample = &samples[instrument - 1];
                let sample_offset = tracker_sample_offset_frames(cell, sample);
                let playback_rate_correction = mod_playback_rate_correction(cell.note, cell.period);
                let key = (channel, instrument, 0, sample_offset);
                let track = track_map.entry(key).or_insert_with(|| {
                    IndexedTrack::new(tracker_instrument(
                        sample,
                        Some(mod_channel_pan(channel)),
                        mix_gain,
                        sample_offset,
                    ))
                });
                apply_tracker_cell_automation(
                    track,
                    note_start_tick,
                    cell,
                    sample.volume,
                    mix_gain,
                    timing,
                    playback_rate_correction,
                );
                active_notes.insert(
                    channel,
                    ActiveTrackerNote {
                        start_index: note_start_tick,
                        instrument,
                        sample_slot: 0,
                        sample_offset,
                        period: cell.period.map(f32::from),
                        playback_rate_correction,
                        note,
                        velocity: cell.volume.unwrap_or(sample.volume),
                    },
                );
                if let Some(period) = cell.period {
                    channel_periods[channel] = Some(f32::from(period));
                }
                channel_volumes[channel] = cell.volume.unwrap_or(sample.volume);
                apply_tracker_channel_volume_effects(
                    &mut track_map,
                    &active_notes,
                    &mut channel_volumes,
                    TrackerVolumeContext {
                        channel,
                        row: global_tick,
                        cell,
                        timing,
                        mix_gain,
                    },
                );
                apply_mod_pitch_effects(
                    &mut track_map,
                    &active_notes,
                    &mut channel_periods,
                    channel,
                    global_tick,
                    cell,
                    timing,
                );
                apply_mod_retrigger_effect(
                    &mut track_map,
                    &mut active_notes,
                    channel,
                    global_tick,
                    cell,
                    timing,
                );
            }
            output_tick = output_tick.saturating_add(u64::from(timing.speed.max(1)));
            if flow_changed {
                break;
            }
        }
        order_index = next_order;
        row_start = next_row_start.min(63);
    }
    close_all_tracker_notes(&mut track_map, &mut active_notes, output_tick);
    for ((channel, instrument, _sample_slot, sample_offset), track) in track_map {
        let track_id = if sample_offset == 0 {
            format!("mod-ch{:02}-inst{:02}", channel + 1, instrument)
        } else {
            format!(
                "mod-ch{:02}-inst{:02}-off{:06}",
                channel + 1,
                instrument,
                sample_offset
            )
        };
        sequence.add_track(TrackId::named(track_id), track);
    }
    let metadata = sequence.metadata.clone();
    Ok(ImportedSequence {
        sequence,
        source_format: ImportedFormat::Mod,
        metadata,
        warnings,
    })
}

fn parse_mod_cell(bytes: &[u8]) -> TrackerCell {
    let period = (u16::from(bytes[0] & 0x0f) << 8) | u16::from(bytes[1]);
    let instrument = usize::from(bytes[0] & 0xf0) | usize::from(bytes[2] >> 4);
    let effect = bytes[2] & 0x0f;
    let effect_param = bytes[3];
    TrackerCell {
        note: mod_period_to_midi(period),
        period: (period > 0).then_some(period),
        instrument,
        volume: None,
        pan: None,
        effect,
        effect_param,
        key_off: effect == 0x0e && (effect_param >> 4) == 0x0c,
    }
}
