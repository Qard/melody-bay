use std::{env, error::Error, fs, thread, time::Duration};

use kira::{AudioManager, AudioManagerSettings, backend::cpal::CpalBackend};
use melody_bay::{ImportedSequence, import_midi, import_mod, import_xm};

const DEFAULT_MIDI: &[u8] = include_bytes!("assets/bach_wtk1_prelude1.mid");
const DEFAULT_MOD: &[u8] = include_bytes!("assets/elektric_funk.mod");
const DEFAULT_XM: &[u8] = include_bytes!("assets/ober1.xm");

fn main() -> Result<(), Box<dyn Error>> {
    let mut manager = AudioManager::<CpalBackend>::new(AudioManagerSettings::default())?;
    let args = env::args().skip(1).collect::<Vec<_>>();
    let imports = if args.is_empty() {
        vec![
            ("midi".to_owned(), import_by_format("midi", DEFAULT_MIDI)?),
            ("mod".to_owned(), import_by_format("mod", DEFAULT_MOD)?),
            ("xm".to_owned(), import_by_format("xm", DEFAULT_XM)?),
        ]
    } else {
        let format = args
            .first()
            .ok_or("missing format: midi | mod | xm")?
            .clone();
        let bytes = if let Some(path) = args.get(1) {
            fs::read(path)?
        } else {
            bundled_bytes(&format)?.to_vec()
        };
        vec![(format.clone(), import_by_format(&format, &bytes)?)]
    };

    for (format, imported) in imports {
        print_summary(&format, &imported);
        let sequence = imported.sequence.resolve();
        let play_seconds = sequence.duration_seconds().clamp(1.5, 8.0);
        let handle = manager.play(sequence.sound_data())?;
        println!("Playing {format} for {play_seconds:.1}s...");
        thread::sleep(Duration::from_secs_f64(play_seconds));
        handle.stop();
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn bundled_bytes(format: &str) -> Result<&'static [u8], Box<dyn Error>> {
    Ok(match format {
        "midi" => DEFAULT_MIDI,
        "mod" => DEFAULT_MOD,
        "xm" => DEFAULT_XM,
        _ => return Err("unknown format: expected midi | mod | xm".into()),
    })
}

fn import_by_format(format: &str, bytes: &[u8]) -> Result<ImportedSequence, Box<dyn Error>> {
    Ok(match format {
        "midi" => import_midi(bytes)?,
        "mod" => import_mod(bytes)?,
        "xm" => import_xm(bytes)?,
        _ => return Err("unknown format: expected midi | mod | xm".into()),
    })
}

fn print_summary(format: &str, imported: &ImportedSequence) {
    println!(
        "{format}: title={:?} composer={:?} tracks={} warnings={}",
        imported.metadata.title,
        imported.metadata.composer,
        imported.sequence.tracks().len(),
        imported.warnings.len()
    );
    for warning in imported.warnings.iter().take(5) {
        println!("  warning: {}", warning.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fallback_assets_import_and_resolve() {
        for (format, bytes) in [
            ("midi", DEFAULT_MIDI),
            ("mod", DEFAULT_MOD),
            ("xm", DEFAULT_XM),
        ] {
            let imported = import_by_format(format, bytes).unwrap();
            assert!(imported.sequence.tracks().len() > 0);
            assert!(imported.sequence.resolve().duration_seconds() > 0.0);
        }
    }
}
