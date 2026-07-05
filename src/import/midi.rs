#[derive(Debug, Clone)]
struct MidiEvent {
    tick: u64,
    order: usize,
    kind: MidiEventKind,
}

#[derive(Debug, Clone)]
enum MidiEventKind {
    NoteOn { ch: u8, note: u8, vel: u8 },
    NoteOff { ch: u8, note: u8 },
    PolyPressure { ch: u8 },
    Program { ch: u8, program: u8 },
    Controller { ch: u8, controller: u8, value: u8 },
    ChannelPressure { ch: u8 },
    PitchBend { ch: u8, value: i16 },
    Tempo { micros_per_quarter: u32 },
    TrackName(String),
    Copyright(String),
    Text { kind: &'static str, text: String },
}

#[derive(Debug, Clone, Copy)]
struct ActiveMidiNote {
    start: u64,
    velocity: u8,
    program: u8,
}

fn import_midi_impl(bytes: &[u8]) -> Result<ImportedSequence, ImportError> {
    if bytes.len() < 14 || &bytes[..4] != b"MThd" {
        return Err(ImportError::InvalidFormat("missing MIDI header"));
    }
    let header_len = be_u32(bytes, 4)? as usize;
    if header_len < 6 || bytes.len() < 8 + header_len {
        return Err(ImportError::InvalidFormat("truncated MIDI header"));
    }
    let format = be_u16(bytes, 8)?;
    let tracks = be_u16(bytes, 10)? as usize;
    let division = be_u16(bytes, 12)?;
    if format > 2 {
        return Err(ImportError::UnsupportedFormatFeature(
            "only MIDI format 0, 1, and 2 are supported",
        ));
    }
    let mut warnings = Vec::new();
    let ppq = if (division & 0x8000) != 0 {
        let ticks_per_frame = u32::from(division & 0x00ff).max(1);
        warnings.push(ImportWarning::approximated_timing(
            ImportedFormat::Midi,
            "SMPTE MIDI time division imported on a ticks-per-frame grid",
        ));
        ticks_per_frame
    } else {
        u32::from(division).max(1)
    };
    if format == 2 {
        warnings.push(ImportWarning::approximated_timing(
            ImportedFormat::Midi,
            "MIDI format 2 independent sequence boundaries were flattened",
        ));
    }
    let mut offset = 8 + header_len;
    let mut events = Vec::new();
    let mut order = 0usize;
    for _ in 0..tracks {
        if offset + 8 > bytes.len() || &bytes[offset..offset + 4] != b"MTrk" {
            return Err(ImportError::InvalidFormat("missing MIDI track chunk"));
        }
        let len = be_u32(bytes, offset + 4)? as usize;
        offset += 8;
        let end = offset
            .checked_add(len)
            .filter(|end| *end <= bytes.len())
            .ok_or(ImportError::InvalidFormat("truncated MIDI track"))?;
        parse_midi_track(&bytes[offset..end], &mut order, &mut events)?;
        offset = end;
    }
    events.sort_by_key(|event| (event.tick, event.order));

    let mut sequence = IndexedSequence::new(ppq);
    let mut programs = [0u8; 16];
    let mut sustain = [false; 16];
    let mut rpn_msb = [None::<u8>; 16];
    let mut rpn_lsb = [None::<u8>; 16];
    let mut pitch_bend_range = [2.0f32; 16];
    let mut active: HashMap<(u8, u8), VecDeque<ActiveMidiNote>> = HashMap::new();
    let mut sustained: HashMap<(u8, u8), VecDeque<ActiveMidiNote>> = HashMap::new();
    let mut tracks_by_lane: HashMap<(u8, u8, Option<u8>), IndexedTrack> = HashMap::new();

    for event in events {
        match event.kind {
            MidiEventKind::Tempo { micros_per_quarter } => {
                if micros_per_quarter == 0 {
                    return Err(ImportError::MalformedTiming("zero MIDI tempo"));
                }
                sequence.tempo_at(event.tick, 60_000_000.0 / f64::from(micros_per_quarter));
            }
            MidiEventKind::Program { ch, program } => programs[ch as usize] = program,
            MidiEventKind::NoteOn { ch, note, vel } if vel > 0 => {
                active
                    .entry((ch, note))
                    .or_default()
                    .push_back(ActiveMidiNote {
                        start: event.tick,
                        velocity: vel,
                        program: programs[ch as usize],
                    });
            }
            MidiEventKind::NoteOn { ch, note, .. } | MidiEventKind::NoteOff { ch, note } => {
                let note_state = active
                    .get_mut(&(ch, note))
                    .and_then(|stack| stack.pop_front());
                if active.get(&(ch, note)).is_some_and(VecDeque::is_empty) {
                    active.remove(&(ch, note));
                }
                if let Some(note_state) = note_state {
                    if sustain[ch as usize] {
                        sustained
                            .entry((ch, note))
                            .or_default()
                            .push_back(note_state);
                    } else {
                        push_midi_note(&mut tracks_by_lane, ch, note, note_state, event.tick);
                    }
                }
            }
            MidiEventKind::Controller {
                ch,
                controller,
                value,
            } => match controller {
                64 => {
                    let was_down = sustain[ch as usize];
                    sustain[ch as usize] = value >= 64;
                    if was_down && value < 64 {
                        let releases = sustained
                            .extract_if(|(note_ch, _), _| *note_ch == ch)
                            .collect::<Vec<_>>();
                        for ((release_ch, note), note_states) in releases {
                            for note_state in note_states {
                                push_midi_note(
                                    &mut tracks_by_lane,
                                    release_ch,
                                    note,
                                    note_state,
                                    event.tick,
                                );
                            }
                        }
                    }
                }
                7 => push_midi_automation(
                    &mut tracks_by_lane,
                    ch,
                    programs[ch as usize],
                    event.tick,
                    "gain",
                    f32::from(value) / 127.0,
                ),
                11 => push_midi_automation(
                    &mut tracks_by_lane,
                    ch,
                    programs[ch as usize],
                    event.tick,
                    "gain",
                    f32::from(value) / 127.0,
                ),
                10 => push_midi_automation(
                    &mut tracks_by_lane,
                    ch,
                    programs[ch as usize],
                    event.tick,
                    "pan",
                    f32::from(value) / 63.5 - 1.0,
                ),
                100 => rpn_lsb[ch as usize] = Some(value),
                101 => rpn_msb[ch as usize] = Some(value),
                6 if rpn_msb[ch as usize] == Some(0) && rpn_lsb[ch as usize] == Some(0) => {
                    pitch_bend_range[ch as usize] = f32::from(value);
                }
                _ => warnings.push(ImportWarning::dropped_controller(
                    ImportedFormat::Midi,
                    controller.to_string(),
                )),
            },
            MidiEventKind::PolyPressure { ch } => warnings.push(ImportWarning::dropped_controller(
                ImportedFormat::Midi,
                format!("poly-pressure channel {}", ch + 1),
            )),
            MidiEventKind::ChannelPressure { ch } => {
                warnings.push(ImportWarning::dropped_controller(
                    ImportedFormat::Midi,
                    format!("channel-pressure channel {}", ch + 1),
                ));
            }
            MidiEventKind::PitchBend { ch, value } => {
                let semitones = f32::from(value) / 8192.0 * pitch_bend_range[ch as usize];
                push_midi_automation(
                    &mut tracks_by_lane,
                    ch,
                    programs[ch as usize],
                    event.tick,
                    "playback_rate",
                    2.0f32.powf(semitones / 12.0),
                );
            }
            MidiEventKind::TrackName(name) => {
                if sequence.metadata.title.is_none() && !name.is_empty() {
                    sequence.metadata.title = Some(name);
                }
            }
            MidiEventKind::Copyright(copyright) => {
                if sequence.metadata.composer.is_none() && !copyright.is_empty() {
                    sequence.metadata.composer = Some(copyright);
                }
            }
            MidiEventKind::Text { kind, text } => warnings.push(ImportWarning::unsupported_event(
                ImportedFormat::Midi,
                format!("{kind}: {text}"),
            )),
        }
    }
    for ((ch, note), note_states) in active.into_iter().chain(sustained) {
        for note_state in note_states {
            push_midi_note(
                &mut tracks_by_lane,
                ch,
                note,
                note_state,
                note_state.start + 1,
            );
        }
    }
    for ((ch, program, drum), track) in tracks_by_lane {
        sequence.add_track(midi_track_id(ch, program, drum), track);
    }
    let metadata = sequence.metadata.clone();
    Ok(ImportedSequence {
        sequence,
        source_format: ImportedFormat::Midi,
        metadata,
        warnings,
    })
}

fn parse_midi_track(
    bytes: &[u8],
    order: &mut usize,
    events: &mut Vec<MidiEvent>,
) -> Result<(), ImportError> {
    let mut offset = 0usize;
    let mut tick = 0u64;
    let mut running = None::<u8>;
    while offset < bytes.len() {
        let delta = read_var_len(bytes, &mut offset)?;
        tick = tick.saturating_add(delta);
        if offset >= bytes.len() {
            return Err(ImportError::InvalidFormat("truncated MIDI event"));
        }
        let mut status = bytes[offset];
        if status < 0x80 {
            status = running.ok_or(ImportError::InvalidFormat(
                "MIDI running status without prior status",
            ))?;
        } else {
            offset += 1;
            if status < 0xf0 {
                running = Some(status);
            }
        }
        match status {
            0x80..=0x8f => {
                let note = read_u8(bytes, &mut offset)?;
                let _velocity = read_u8(bytes, &mut offset)?;
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::NoteOff {
                        ch: status & 0x0f,
                        note,
                    },
                );
            }
            0x90..=0x9f => {
                let note = read_u8(bytes, &mut offset)?;
                let vel = read_u8(bytes, &mut offset)?;
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::NoteOn {
                        ch: status & 0x0f,
                        note,
                        vel,
                    },
                );
            }
            0xb0..=0xbf => {
                let controller = read_u8(bytes, &mut offset)?;
                let value = read_u8(bytes, &mut offset)?;
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::Controller {
                        ch: status & 0x0f,
                        controller,
                        value,
                    },
                );
            }
            0xc0..=0xcf => {
                let program = read_u8(bytes, &mut offset)?;
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::Program {
                        ch: status & 0x0f,
                        program,
                    },
                );
            }
            0xe0..=0xef => {
                let lsb = read_u8(bytes, &mut offset)?;
                let msb = read_u8(bytes, &mut offset)?;
                let raw = (i16::from(msb) << 7) | i16::from(lsb);
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::PitchBend {
                        ch: status & 0x0f,
                        value: raw - 8192,
                    },
                );
            }
            0xa0..=0xaf => {
                let _note = read_u8(bytes, &mut offset)?;
                let _pressure = read_u8(bytes, &mut offset)?;
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::PolyPressure { ch: status & 0x0f },
                );
            }
            0xd0..=0xdf => {
                let _pressure = read_u8(bytes, &mut offset)?;
                push_midi_event(
                    events,
                    order,
                    tick,
                    MidiEventKind::ChannelPressure { ch: status & 0x0f },
                );
            }
            0xff => {
                let meta = read_u8(bytes, &mut offset)?;
                let len = read_var_len(bytes, &mut offset)? as usize;
                let end = offset
                    .checked_add(len)
                    .filter(|end| *end <= bytes.len())
                    .ok_or(ImportError::InvalidFormat("truncated MIDI meta event"))?;
                push_midi_meta_event(events, order, tick, meta, &bytes[offset..end]);
                offset = end;
            }
            0xf0 | 0xf7 => {
                let len = read_var_len(bytes, &mut offset)? as usize;
                offset = offset
                    .checked_add(len)
                    .filter(|end| *end <= bytes.len())
                    .ok_or(ImportError::InvalidFormat("truncated MIDI sysex event"))?;
            }
            _ => return Err(ImportError::InvalidFormat("unknown MIDI event status")),
        }
    }
    Ok(())
}

fn push_midi_event(events: &mut Vec<MidiEvent>, order: &mut usize, tick: u64, kind: MidiEventKind) {
    events.push(MidiEvent {
        tick,
        order: *order,
        kind,
    });
    *order += 1;
}

fn push_midi_meta_event(
    events: &mut Vec<MidiEvent>,
    order: &mut usize,
    tick: u64,
    meta: u8,
    data: &[u8],
) {
    let kind = match meta {
        0x51 if data.len() == 3 => Some(MidiEventKind::Tempo {
            micros_per_quarter: (u32::from(data[0]) << 16)
                | (u32::from(data[1]) << 8)
                | u32::from(data[2]),
        }),
        0x02 => Some(MidiEventKind::Copyright(trim_c_string(data))),
        0x03 => Some(MidiEventKind::TrackName(trim_c_string(data))),
        0x01 => Some(MidiEventKind::Text {
            kind: "text",
            text: trim_c_string(data),
        }),
        0x05 => Some(MidiEventKind::Text {
            kind: "lyric",
            text: trim_c_string(data),
        }),
        0x06 => Some(MidiEventKind::Text {
            kind: "marker",
            text: trim_c_string(data),
        }),
        0x07 => Some(MidiEventKind::Text {
            kind: "cue",
            text: trim_c_string(data),
        }),
        0x58 => Some(MidiEventKind::Text {
            kind: "time-signature",
            text: hex_bytes(data),
        }),
        0x59 => Some(MidiEventKind::Text {
            kind: "key-signature",
            text: hex_bytes(data),
        }),
        _ => None,
    };
    if let Some(kind) = kind {
        push_midi_event(events, order, tick, kind);
    }
}

fn push_midi_note(
    tracks: &mut HashMap<(u8, u8, Option<u8>), IndexedTrack>,
    ch: u8,
    note: u8,
    state: ActiveMidiNote,
    end_tick: u64,
) {
    let drum_note = (ch == 9).then_some(note);
    let key = (ch, state.program, drum_note);
    let instrument = if let Some(drum) = drum_note {
        gm_drum(drum)
    } else {
        gm_instrument(state.program)
    };
    let track = tracks
        .entry(key)
        .or_insert_with(|| IndexedTrack::new(instrument));
    track.notes.push(IndexedNoteEvent {
        start_index: state.start,
        duration_indices: end_tick.saturating_sub(state.start).max(1),
        note: Note::from_midi(note),
        velocity: Velocity::new(f32::from(state.velocity) / 127.0),
    });
}

fn push_midi_automation(
    tracks: &mut HashMap<(u8, u8, Option<u8>), IndexedTrack>,
    ch: u8,
    program: u8,
    tick: u64,
    target: &str,
    value: f32,
) {
    let key = (ch, program, None);
    let track = tracks
        .entry(key)
        .or_insert_with(|| IndexedTrack::new(gm_instrument(program)));
    track.automation.push(IndexedAutomationEvent {
        index: tick,
        target: target.to_owned(),
        shape: IndexedAutomationShape::SetValue { value },
    });
}

fn midi_track_id(ch: u8, program: u8, drum_note: Option<u8>) -> TrackId {
    if let Some(note) = drum_note {
        TrackId::named(format!("midi-ch{:02}-drum{:03}", ch + 1, note))
    } else {
        TrackId::named(format!("midi-ch{:02}-program{:03}", ch + 1, program))
    }
}

