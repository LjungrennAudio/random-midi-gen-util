use clap::{Parser, ValueEnum};
use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::fs;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ScaleOpt {
    Major,
    NaturalMinor,
    MinorPentatonic,
    MajorPentatonic,
}

#[derive(Debug, Parser)]
#[command(name = "midi-seed-gen", version, about = "Seeded random MIDI (format 0) generator")]
struct Cli {
    /// Output .mid path
    #[arg(short, long, default_value = "out/seeded.mid")]
    out: String,

    /// RNG seed (same seed => same MIDI)
    #[arg(long, default_value_t = 0xC0FFEEu64)]
    seed: u64,

    /// Tempo in BPM
    #[arg(long, default_value_t = 120u32)]
    bpm: u32,

    /// Bars (assumes 4/4)
    #[arg(long, default_value_t = 16u32)]
    bars: u32,

    /// Ticks per quarter note (PPQN)
    #[arg(long, default_value_t = 480u16)]
    ppqn: u16,

    /// Root MIDI note number (60=C4)
    #[arg(long, default_value_t = 60u8)]
    root: u8,

    /// Scale / mode
    #[arg(long, value_enum, default_value_t = ScaleOpt::MinorPentatonic)]
    scale: ScaleOpt,

    /// MIDI channel (0..15). (Channel 9 is the “10th channel” used for drums in GM practice.)
    #[arg(long, default_value_t = 0u8)]
    channel: u8,

    /// Program (0..127). 0 = Acoustic Grand Piano in General MIDI.
    #[arg(long, default_value_t = 0u8)]
    program: u8,
}

fn bpm_to_us_per_quarter(bpm: u32) -> u32 {
    // 60_000_000 microseconds per minute / bpm
    60_000_000u32 / bpm.max(1)
}

fn scale_semitones(s: ScaleOpt) -> &'static [i8] {
    match s {
        ScaleOpt::Major => &[0, 2, 4, 5, 7, 9, 11],
        ScaleOpt::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
        ScaleOpt::MinorPentatonic => &[0, 3, 5, 7, 10],
        ScaleOpt::MajorPentatonic => &[0, 2, 4, 7, 9],
    }
}

fn weighted_choice<R: Rng>(rng: &mut R, items: &[(u8, u32)]) -> u8 {
    let total: u32 = items.iter().map(|(_, w)| *w).sum();
    let mut x = rng.gen_range(0..total.max(1));
    for (v, w) in items {
        if x < *w {
            return *v;
        }
        x -= *w;
    }
    items.last().unwrap().0
}

fn event_order_key(kind: &TrackEventKind) -> u8 {
    match kind {
        TrackEventKind::Midi { message, .. } => match message {
            MidiMessage::NoteOff { .. } => 0,
            MidiMessage::NoteOn { .. } => 1,
            _ => 2,
        },
        TrackEventKind::Meta(_) => 3,
        TrackEventKind::SysEx(_) | TrackEventKind::Escape(_) => 4,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse(); // clap derive parsing [web:68]

    // Basic argument sanity
    if cli.channel > 15 {
        return Err("channel must be 0..15".into());
    }
    if cli.program > 127 {
        return Err("program must be 0..127".into());
    }

    let mut rng = ChaCha8Rng::seed_from_u64(cli.seed);

    let scale = scale_semitones(cli.scale);
    let base_note = cli.root as i16;

    // 4/4 grid: 16 steps per bar => 1/16 note per step.
    let steps_per_bar = 16u32;
    let step_ticks: u32 = (cli.ppqn as u32) / 4; // 1/16 = quarter/4
    let total_steps: u32 = cli.bars * steps_per_bar;
    let song_len_ticks: u32 = total_steps * step_ticks;

    // Collect events as (absolute_tick, kind)
    let mut abs_events: Vec<(u32, TrackEventKind)> = Vec::new();

    // Tempo at time 0: Set Tempo meta message (µs per quarter note). [web:14][web:91]
    let us_per_qn = bpm_to_us_per_quarter(cli.bpm);
    abs_events.push((
        0,
        TrackEventKind::Meta(MetaMessage::Tempo(us_per_qn.into())),
    ));

    // Program change at time 0 (instrument selection). [web:91]
    abs_events.push((
        0,
        TrackEventKind::Midi {
            channel: cli.channel.into(),
            message: MidiMessage::ProgramChange {
                program: cli.program.into(),
            },
        },
    ));

    // Stochastic melody with light constraints.
    let mut last_degree: i32 = 0;
    for step in 0..total_steps {
        let t0 = step * step_ticks;

        // Note density: 45% notes, 55% rests.
        if rng.gen_range(0..100u32) < 55 {
            continue;
        }

        // Choose a degree index in 0..scale.len()
        // Slightly bias toward "stable-ish" degrees (0 and 2 if they exist).
        let max_deg = (scale.len() as i32).max(1);
        let target = if max_deg >= 3 {
            weighted_choice(&mut rng, &[(0, 30), (1, 15), (2, 30), (3, 15), (4, 10)]) as i32
        } else {
            rng.gen_range(0..max_deg as u32) as i32
        };
        let target = target.clamp(0, max_deg - 1);

        // Encourage stepwise motion 65% of the time.
        let deg = if rng.gen_range(0..100u32) < 65 {
            let delta = match rng.gen_range(0..3u32) {
                0 => -1,
                1 => 0,
                _ => 1,
            };
            (last_degree + delta).clamp(0, max_deg - 1)
        } else {
            target
        };
        last_degree = deg;

        let semis = scale[deg as usize] as i16;

        // Occasionally jump an octave up/down (but keep in MIDI 0..127).
        let octave_shift: i16 = match rng.gen_range(0..100u32) {
            0..=9 => 12,
            10..=14 => -12,
            _ => 0,
        };

        let note_i16 = base_note + semis + octave_shift;
        let note_u8 = note_i16.clamp(0, 127) as u8;

        // Duration in steps (1..4) with weights.
        let dur_steps: u32 = weighted_choice(&mut rng, &[(1, 40), (2, 30), (3, 10), (4, 20)]) as u32;

        let t1 = (t0 + dur_steps * step_ticks).min(song_len_ticks);

        // Velocity: accent on quarter-note boundaries.
        let accent: u8 = if step % 4 == 0 { 18 } else { 0 };
        let vel: u8 = (rng.gen_range(55..95) as u16 + accent as u16).min(127) as u8;

        abs_events.push((
            t0,
            TrackEventKind::Midi {
                channel: cli.channel.into(),
                message: MidiMessage::NoteOn {
                    key: note_u8.into(),
                    vel: vel.into(),
                },
            },
        ));

        abs_events.push((
            t1,
            TrackEventKind::Midi {
                channel: cli.channel.into(),
                message: MidiMessage::NoteOff {
                    key: note_u8.into(),
                    vel: 0.into(),
                },
            },
        ));
    }

    // Sort and ensure sensible ordering at identical timestamps.
    abs_events.sort_by(|(ta, ea), (tb, eb)| {
        ta.cmp(tb).then_with(|| event_order_key(ea).cmp(&event_order_key(eb)))
    });

    // Convert abs->delta and build the single track. [web:91]
    let mut track: Vec<TrackEvent> = Vec::new();
    let mut last_tick: u32 = 0;
    for (tick, kind) in abs_events {
        let delta = tick.saturating_sub(last_tick);
        last_tick = tick;
        track.push(TrackEvent {
            delta: (delta as u32).into(),
            kind,
        });
    }

    // End-of-track meta event must be last. [web:91]
    track.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    // Format 0 = SingleTrack in midly. [web:36]
    let header = Header::new(Format::SingleTrack, Timing::Metrical(cli.ppqn.into()));
    let smf = Smf {
        header,
        tracks: vec![track],
    };

    // Write file
    if let Some(parent) = std::path::Path::new(&cli.out).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    smf.save(&cli.out)?; // midly save helper [web:36]

    eprintln!("Wrote {}", cli.out);
    Ok(())
}
