fn import_xm_impl(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
    if bytes.len() < 80 || &bytes[..17] != b"Extended Module: " || bytes[37] != 0x1a {
        return Err(ImportError::InvalidFormat("missing XM header"));
    }
    let title = trim_c_string(&bytes[17..37]);
    let header_size = le_u32(bytes, 60)? as usize;
    if header_size < 20 || 60 + header_size > bytes.len() {
        return Err(ImportError::InvalidFormat("truncated XM song header"));
    }
    let song_len = usize::from(le_u16(bytes, 64)?).max(1);
    let channels = usize::from(le_u16(bytes, 68)?).max(1);
    let patterns = usize::from(le_u16(bytes, 70)?);
    let instruments = usize::from(le_u16(bytes, 72)?);
    let speed = u32::from(le_u16(bytes, 76)?).max(1);
    let bpm = u32::from(le_u16(bytes, 78)?).max(1);
    let envelope_tick_seconds = 2.5 / f64::from(bpm);
    let orders = &bytes[80..80 + 256];
    let mut offset = 60 + header_size;
    let mut pattern_rows = Vec::<Vec<Vec<TrackerCell>>>::new();
    for _ in 0..patterns {
        let header_len = le_u32(bytes, offset)? as usize;
        if header_len < 9 || offset + header_len > bytes.len() {
            return Err(ImportError::InvalidFormat("truncated XM pattern header"));
        }
        let rows = usize::from(le_u16(bytes, offset + 5)?).max(1);
        let packed_len = usize::from(le_u16(bytes, offset + 7)?);
        offset += header_len;
        let end = offset
            .checked_add(packed_len)
            .filter(|end| *end <= bytes.len())
            .ok_or(ImportError::InvalidFormat("truncated XM pattern data"))?;
        pattern_rows.push(parse_xm_pattern(&bytes[offset..end], rows, channels)?);
        offset = end;
    }
    let mut samples = Vec::with_capacity(instruments);
    for _instrument_index in 0..instruments {
        let header_len = le_u32(bytes, offset)? as usize;
        if header_len < 29 || offset + header_len > bytes.len() {
            return Err(ImportError::InvalidFormat("truncated XM instrument header"));
        }
        let instrument_header_end = offset + header_len;
        let sample_count = usize::from(le_u16(bytes, offset + 27)?);
        let sample_header_size = if sample_count > 0 {
            le_u32(bytes, offset + 29)? as usize
        } else {
            0
        };
        let sample_map_start = offset + 33;
        let volume_env_start = sample_map_start + 96;
        let envelope_counts_start = volume_env_start + 96;
        let volume_type_offset = envelope_counts_start + 8;
        let volume_points = if sample_count > 0
            && volume_env_start + 48 <= offset + header_len
            && envelope_counts_start < offset + header_len
            && bytes.get(volume_type_offset).copied().unwrap_or(0) & 0x01 != 0
        {
            parse_xm_envelope(
                &bytes[volume_env_start..volume_env_start + 48],
                bytes.get(envelope_counts_start).copied().unwrap_or(0),
                envelope_tick_seconds,
            )
        } else {
            None
        };
        offset = instrument_header_end;
        let mut keymap = [0usize; 96];
        if sample_count > 0 && sample_map_start + 96 <= instrument_header_end {
            for (slot, value) in bytes[sample_map_start..sample_map_start + 96]
                .iter()
                .enumerate()
            {
                keymap[slot] = usize::from(*value);
            }
        }
        let mut instrument_samples = Vec::new();
        for _ in 0..sample_count {
            if sample_header_size < 40 || offset + sample_header_size > bytes.len() {
                return Err(ImportError::InvalidFormat("truncated XM sample header"));
            }
            let length = le_u32(bytes, offset)? as usize;
            let loop_start = le_u32(bytes, offset + 4)? as usize;
            let loop_len = le_u32(bytes, offset + 8)? as usize;
            let volume = f32::from(bytes[offset + 12].min(64)) / 64.0;
            let finetune = i8::from_ne_bytes([bytes[offset + 13]]);
            let sample_type = bytes[offset + 14];
            let is_16_bit = sample_type & 0x10 != 0;
            let loop_type = sample_type & 0x03;
            let pan = f32::from(bytes[offset + 15]) / 127.5 - 1.0;
            let relative_note = i8::from_ne_bytes([bytes[offset + 16]]);
            let name_start = offset + 18;
            let name_end = (name_start + 22).min(offset + sample_header_size);
            let name = trim_c_string(&bytes[name_start..name_end]);
            instrument_samples.push(TrackerSample {
                _name: name,
                length,
                is_16_bit,
                volume,
                finetune_cents: f32::from(finetune) * 100.0 / 128.0,
                loop_start,
                loop_len: if loop_type == 0 { 0 } else { loop_len },
                ping_pong_loop: loop_type == 0x02,
                data: Vec::new(),
                relative_note,
                pan,
                envelope: volume_points.clone(),
            });
            offset += sample_header_size;
        }
        for sample in &mut instrument_samples {
            let end = offset
                .checked_add(sample.length)
                .filter(|end| *end <= bytes.len())
                .ok_or(ImportError::MalformedSampleData("truncated XM sample data"))?;
            if sample.is_16_bit {
                if sample.length % 2 != 0 {
                    return Err(ImportError::MalformedSampleData(
                        "odd XM 16-bit sample byte length",
                    ));
                }
                let mut acc = 0i16;
                sample.data = bytes[offset..end]
                    .chunks_exact(2)
                    .map(|delta| {
                        let delta = i16::from_le_bytes([delta[0], delta[1]]);
                        acc = acc.wrapping_add(delta);
                        f32::from(acc) / 32768.0
                    })
                    .collect();
                sample.loop_start /= 2;
                sample.loop_len /= 2;
            } else {
                let mut acc = 0i8;
                sample.data = bytes[offset..end]
                    .iter()
                    .map(|delta| {
                        acc = acc.wrapping_add(i8::from_ne_bytes([*delta]));
                        f32::from(acc) / 128.0
                    })
                    .collect();
            }
            expand_ping_pong_loop(sample);
            offset = end;
        }
        samples.push(XmInstrument {
            keymap,
            samples: instrument_samples,
        });
    }

    let mut timing = TrackerTiming { speed, bpm };
    let mut sequence = IndexedSequence::new(24).title(title);
    sequence.tempo_at(0, f64::from(timing.bpm));
    let mut track_map = BTreeMap::<(usize, usize, usize, usize), IndexedTrack>::new();
    let mut active_notes = BTreeMap::<usize, ActiveTrackerNote>::new();
    let mut channel_instruments = vec![0usize; channels];
    let mut channel_volumes = vec![1.0f32; channels];
    let mut warnings = Vec::new();
    let mut output_tick = 0u64;
    let mut order_index = 0usize;
    let mut row_start = 0usize;
    let mut flow_guard = 0usize;
    let mix_gain = tracker_mix_gain(channels);
    while order_index < song_len && flow_guard < song_len.saturating_mul(256).max(1) {
        flow_guard += 1;
        let pattern_order = orders[order_index];
        let Some(rows) = pattern_rows.get(usize::from(pattern_order)) else {
            warnings.push(ImportWarning::new("XM order references missing pattern"));
            order_index += 1;
            continue;
        };
        let mut next_order = order_index + 1;
        let mut next_row_start = 0usize;
        let mut flow_changed = false;
        for row in rows.iter().skip(row_start) {
            let global_tick = output_tick;
            for (channel, cell) in row.iter().copied().enumerate() {
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
                    ImportedFormat::Xm,
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
                    continue;
                };
                close_tracker_channel_note(&mut track_map, &mut active_notes, channel, global_tick);
                let instrument_number = if cell.instrument > 0 {
                    cell.instrument
                } else {
                    channel_instruments[channel]
                };
                if instrument_number == 0 || instrument_number > samples.len() {
                    warnings.push(ImportWarning::new("XM note without valid instrument"));
                    continue;
                }
                let instrument = &samples[instrument_number - 1];
                let (sample_slot, sample) = xm_instrument_sample(instrument, note).ok_or(
                    ImportError::MalformedSampleData("XM instrument has no sample data"),
                )?;
                let sample_offset = tracker_sample_offset_frames(cell, sample);
                let track = track_map
                    .entry((channel, instrument_number, sample_slot, sample_offset))
                    .or_insert_with(|| {
                        IndexedTrack::new(tracker_instrument(
                            sample,
                            xm_channel_pan(channel, sample),
                            mix_gain,
                            sample_offset,
                        ))
                    });
                apply_tracker_cell_automation(
                    track,
                    global_tick,
                    cell,
                    sample.volume,
                    mix_gain,
                    timing,
                    1.0,
                );
                active_notes.insert(
                    channel,
                    ActiveTrackerNote {
                        start_index: global_tick,
                        instrument: instrument_number,
                        sample_slot,
                        sample_offset,
                        period: None,
                        playback_rate_correction: 1.0,
                        note,
                        velocity: cell.volume.unwrap_or(sample.volume),
                    },
                );
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
            }
            output_tick = output_tick.saturating_add(u64::from(timing.speed.max(1)));
            if flow_changed {
                break;
            }
        }
        order_index = next_order;
        row_start = next_row_start.min(255);
    }
    close_all_tracker_notes(&mut track_map, &mut active_notes, output_tick);
    for ((channel, instrument, sample_slot, sample_offset), track) in track_map {
        let mut track_id = if sample_slot == 0 {
            format!("xm-ch{:02}-inst{:02}", channel + 1, instrument)
        } else {
            format!(
                "xm-ch{:02}-inst{:02}-samp{:02}",
                channel + 1,
                instrument,
                sample_slot + 1
            )
        };
        if sample_offset > 0 {
            track_id.push_str(&format!("-off{sample_offset:06}"));
        }
        sequence.add_track(TrackId::named(track_id), track);
    }
    let metadata = sequence.metadata.clone();
    Ok(ImportedSequence {
        sequence,
        source_format: ImportedFormat::Xm,
        metadata,
        warnings,
    })
}

fn parse_xm_pattern(
    bytes: &[u8],
    rows: usize,
    channels: usize,
) -> Result<Vec<Vec<TrackerCell>>, ImportError> {
    let mut offset = 0usize;
    let mut parsed = Vec::with_capacity(rows);
    for _ in 0..rows {
        let mut row = Vec::with_capacity(channels);
        for _ in 0..channels {
            let first = read_u8(bytes, &mut offset)?;
            let (note, instrument, volume, effect, effect_param) = if first & 0x80 != 0 {
                let note = if first & 0x01 != 0 {
                    read_u8(bytes, &mut offset)?
                } else {
                    0
                };
                let instrument = if first & 0x02 != 0 {
                    read_u8(bytes, &mut offset)?
                } else {
                    0
                };
                let volume = if first & 0x04 != 0 {
                    read_u8(bytes, &mut offset)?
                } else {
                    0
                };
                let effect = if first & 0x08 != 0 {
                    read_u8(bytes, &mut offset)?
                } else {
                    0
                };
                let param = if first & 0x10 != 0 {
                    read_u8(bytes, &mut offset)?
                } else {
                    0
                };
                (note, instrument, volume, effect, param)
            } else {
                (
                    first,
                    read_u8(bytes, &mut offset)?,
                    read_u8(bytes, &mut offset)?,
                    read_u8(bytes, &mut offset)?,
                    read_u8(bytes, &mut offset)?,
                )
            };
            row.push(TrackerCell {
                note: xm_note_to_midi(note),
                period: None,
                instrument: usize::from(instrument),
                volume: xm_volume_to_velocity(volume),
                pan: xm_volume_to_pan(volume),
                effect,
                effect_param,
                key_off: note == 97 || effect == 0x14,
            });
        }
        parsed.push(row);
    }
    Ok(parsed)
}

fn parse_xm_envelope(bytes: &[u8], count: u8, tick_seconds: f64) -> Option<SampleEnvelope> {
    let count = usize::from(count).min(12);
    if count == 0 {
        return None;
    }
    let tick_seconds = if tick_seconds.is_finite() && tick_seconds > 0.0 {
        tick_seconds
    } else {
        0.02
    };
    let mut points = Vec::with_capacity(count);
    for index in 0..count {
        let offset = index * 4;
        let x = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as f64 * tick_seconds;
        let y = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as f32 / 64.0;
        points.push((x, y));
    }
    Some(SampleEnvelope { points })
}
