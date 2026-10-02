//! MW2's own menu music and UI sounds, read from the player's install at
//! runtime: the main-menu theme streamed from `main/iw_*.iwd`, the hover and
//! click blips from the localized post-gfx zone. Nothing is shipped.
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use asset_audio::{AssetNamespace, IwdSoundIndex, MSS_PCM, SoundCatalog};
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};

const ZONE: &str = "localized_code_post_gfx_mp.ff";
const MUSIC: &str = "music_mainmenu_mp";
const HOVER: &str = "mouse_over";
const CLICK: &str = "mouse_click";

/// Seconds for the music to fade in once decoded or when the game exits.
const FADE_IN: f32 = 1.5;
/// Seconds for the music to fade out on game launch or when muted.
const FADE_OUT: f32 = 0.8;

/// Decoded interleaved samples plus the alias's own mix settings.
#[derive(Debug)]
pub struct Clip {
    pub channels: u16,
    pub rate: u32,
    pub samples: Vec<f32>,
    /// The alias's `vol_min`.
    pub volume: f32,
    /// The alias's `pitch_min`, played back as a speed ratio.
    pub pitch: f32,
}

impl Clip {
    #[cfg(test)]
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.channels) / f64::from(self.rate)
    }
}

/// Whatever of the three sounds could be read; any may be missing.
#[derive(Debug, Default)]
pub struct MenuSounds {
    pub music: Option<Clip>,
    pub hover: Option<Clip>,
    pub click: Option<Clip>,
}

/// `<mw2>/zone/english/…` first, else the first language folder that has
/// the zone.
fn zone_path(mw2: &Path) -> Option<PathBuf> {
    let zone = mw2.join("zone");
    let english = zone.join("english").join(ZONE);
    if english.is_file() {
        return Some(english);
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&zone)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(ZONE))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

/// The alias's first variant's volume and pitch.
fn mix_of(catalog: &SoundCatalog, alias: &str) -> Option<(f32, f32)> {
    let row = catalog
        .sound_in(AssetNamespace::Iw4, alias)?
        .aliases
        .first()?;
    Some((row.vol_min, row.pitch_min))
}

/// A sound held in the zone as 16-bit MSS PCM.
fn loaded_clip(catalog: &SoundCatalog, alias: &str) -> Option<Clip> {
    let (volume, pitch) = mix_of(catalog, alias)?;
    let row = catalog
        .sound_in(AssetNamespace::Iw4, alias)?
        .aliases
        .first()?;
    let pcm = catalog.pcm_at(row.loaded.bound_index()?)?;
    if pcm.format() != MSS_PCM || pcm.bits() != 16 {
        return None;
    }
    let channels = u16::try_from(pcm.channels()).ok().filter(|&c| c > 0)?;
    let samples = pcm
        .encoded_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| f32::from(i16::from_le_bytes(pair)) / 32768.0)
        .collect();
    Some(Clip {
        channels,
        rate: pcm.rate,
        samples,
        volume,
        pitch,
    })
}

/// A sound streamed from the IWDs (an MP3 for the menu music).
fn streamed_clip(catalog: &SoundCatalog, main: &Path, alias: &str) -> Result<Clip, String> {
    let (volume, pitch) = mix_of(catalog, alias).ok_or(format!("no alias {alias}"))?;
    let (_, dir, name) = catalog
        .streamed_for_variant(AssetNamespace::Iw4, alias, 0)
        .ok_or(format!("{alias} is not streamed"))?;
    let relative = format!("{dir}/{name}");
    let bytes = IwdSoundIndex::open(main)?
        .read_sound(&relative)
        .ok_or(format!("sound/{relative} is in no IWD"))??;
    let (channels, rate, samples) =
        decode_compressed(&bytes).ok_or(format!("cannot decode sound/{relative}"))?;
    Ok(Clip {
        channels,
        rate,
        samples,
        volume,
        pitch,
    })
}

/// Decodes a whole MP3 to interleaved `f32`.
fn decode_compressed(bytes: &[u8]) -> Option<(u16, u32, Vec<f32>)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let cursor = std::io::Cursor::new(bytes.to_vec());
    // Trim the encoder delay and padding (from the LAME tag) so the loop
    // point has no silent gap.
    let format_options = FormatOptions {
        enable_gapless: true,
        ..FormatOptions::default()
    };
    let stream = MediaSourceStream::new(Box::new(cursor), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &format_options, &MetadataOptions::default())
        .ok()?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)?;
    let track_id = track.id;
    let rate = track.codec_params.sample_rate?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;
    let mut samples = Vec::new();
    let mut channels: u16 = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::ResetRequired) => continue,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        channels = channels.max(decoded.spec().channels.count() as u16);
        let mut interleaved = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        interleaved.copy_interleaved_ref(decoded);
        samples.extend_from_slice(interleaved.samples());
    }
    (!samples.is_empty() && channels > 0).then_some((channels, rate, samples))
}

/// Reads the three menu sounds from an MW2 install. Fails only when the
/// zone can't be found or parsed; a missing sound is just `None`.
pub fn load(mw2: &Path) -> Result<MenuSounds, String> {
    let zone = zone_path(mw2).ok_or(format!("no zone/*/{ZONE} in {}", mw2.display()))?;
    let mut catalog = asset_audio::load_sound_catalog(&zone)?;
    catalog.finalize();
    let music = match streamed_clip(&catalog, &mw2.join("main"), MUSIC) {
        Ok(clip) => Some(clip),
        Err(error) => {
            eprintln!("menu music unavailable: {error}");
            None
        }
    };
    Ok(MenuSounds {
        music,
        hover: loaded_clip(&catalog, HOVER),
        click: loaded_clip(&catalog, CLICK),
    })
}

/// What the loader thread hands back: the sounds, and an output device
/// opened there so the UI thread never waits on the audio backend.
pub struct Loaded {
    pub sounds: Result<MenuSounds, String>,
    /// `None` when not asked for, when loading failed, or when no device
    /// could be opened (the launcher then stays silent).
    pub output: Option<MixerDeviceSink>,
}

/// `load` on a worker thread, which also opens the default output device
/// when `open_output` is set; the result arrives once.
pub fn start_loading(mw2: PathBuf, open_output: bool) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let sounds = load(&mw2);
        let output = if open_output && sounds.is_ok() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    Some(sink)
                }
                Err(error) => {
                    eprintln!("no audio output, launcher stays silent: {error}");
                    None
                }
            }
        } else {
            None
        };
        let _ = tx.send(Loaded { sounds, output });
    });
    rx
}

/// Moves `current` one step of at most `step` towards `target`.
pub fn step_gain(current: f32, target: f32, step: f32) -> f32 {
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

/// Smallest distance a fade's rate is computed from (see `fade_to`).
const MIN_FADE_DISTANCE: f32 = 1e-3;

/// Fade state shared between the UI thread and a `GainRamp` running on
/// the audio thread. Gains are 0..1 stored as `f32` bits.
#[derive(Debug, Default)]
pub struct RampControl {
    target: AtomicU32,
    /// Gain change per second.
    rate: AtomicU32,
    /// The gain the audio thread last applied.
    current: AtomicU32,
}

impl RampControl {
    /// Fades from wherever the gain is now to `target` over `seconds`; a
    /// new fade replaces one in progress.
    pub fn fade_to(&self, target: f32, seconds: f32) {
        // A floor on the distance: a fade set (almost) exactly at its target
        // would otherwise get a near-zero rate and leave a tiny residue that
        // never closes, so `tick` would never see the gain reach 0.
        let distance = (target - self.current()).abs().max(MIN_FADE_DISTANCE);
        let rate = if seconds > 0.0 {
            distance / seconds
        } else {
            f32::INFINITY
        };
        // Rate first, then target with Release: the audio thread loads the
        // target with Acquire, so it never pairs a new target with the old
        // rate.
        self.rate.store(rate.to_bits(), Ordering::Release);
        self.target.store(target.to_bits(), Ordering::Release);
    }

    pub fn target(&self) -> f32 {
        f32::from_bits(self.target.load(Ordering::Acquire))
    }

    fn rate(&self) -> f32 {
        f32::from_bits(self.rate.load(Ordering::Acquire))
    }

    pub fn current(&self) -> f32 {
        f32::from_bits(self.current.load(Ordering::Relaxed))
    }
}

/// Scales `inner` by a gain that ramps towards `RampControl`'s target one
/// frame at a time, so fades run on the audio thread whether or not the
/// launcher is drawing frames.
pub struct GainRamp<S> {
    inner: S,
    control: Arc<RampControl>,
    gain: f32,
    /// Position within the current frame; the gain steps once per frame.
    channel: u16,
}

impl<S: Source> GainRamp<S> {
    pub fn new(inner: S, control: Arc<RampControl>) -> Self {
        let gain = control.current();
        Self {
            inner,
            control,
            gain,
            channel: 0,
        }
    }
}

impl<S: Source> Iterator for GainRamp<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        let sample = self.inner.next()?;
        if self.channel == 0 {
            let target = self.control.target();
            if self.gain != target {
                let step = self.control.rate() / self.inner.sample_rate().get() as f32;
                self.gain = step_gain(self.gain, target, step);
                self.control
                    .current
                    .store(self.gain.to_bits(), Ordering::Relaxed);
            }
        }
        self.channel = (self.channel + 1) % self.inner.channels().get();
        Some(sample * self.gain as rodio::Sample)
    }
}

impl<S: Source> Source for GainRamp<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// Endless playback of one shared buffer, wrapping at the end, so the music
/// is held once (unlike `repeat_infinite`, which buffers a second copy).
pub struct Looping {
    data: Arc<[rodio::Sample]>,
    pos: usize,
    channels: rodio::ChannelCount,
    rate: rodio::SampleRate,
}

impl Looping {
    pub fn new(
        channels: rodio::ChannelCount,
        rate: rodio::SampleRate,
        data: Arc<[rodio::Sample]>,
    ) -> Self {
        Self {
            data,
            pos: 0,
            channels,
            rate,
        }
    }
}

impl Iterator for Looping {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        let sample = *self.data.get(self.pos)?;
        self.pos = (self.pos + 1) % self.data.len();
        Some(sample)
    }
}

impl Source for Looping {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// A short sound kept ready to play; cloning the buffer shares its samples.
struct Cue {
    buffer: SamplesBuffer,
    volume: f32,
    pitch: f32,
}

impl Cue {
    fn new(clip: Clip) -> Option<Self> {
        let channels = NonZero::new(clip.channels)?;
        let rate = NonZero::new(clip.rate)?;
        let samples: Vec<rodio::Sample> = clip
            .samples
            .into_iter()
            .map(|s| s as rodio::Sample)
            .collect();
        Some(Self {
            buffer: SamplesBuffer::new(channels, rate, samples),
            volume: clip.volume,
            pitch: clip.pitch,
        })
    }
}

struct Music {
    player: Player,
    ramp: Arc<RampControl>,
}

/// The audio output and its players. The output device arrives from the
/// loader thread; without one, everything is silent.
pub struct MenuAudio {
    /// Must outlive every player: dropping it stops all sound.
    output: Option<MixerDeviceSink>,
    music: Option<Music>,
    hover: Option<Cue>,
    click: Option<Cue>,
    /// Replaced on each hover, which stops the one before.
    hover_player: Option<Player>,
    click_player: Option<Player>,
    music_enabled: bool,
    ui_enabled: bool,
    in_game: bool,
}

impl MenuAudio {
    pub fn new(music_enabled: bool, ui_enabled: bool) -> Self {
        Self {
            output: None,
            music: None,
            hover: None,
            click: None,
            hover_player: None,
            click_player: None,
            music_enabled,
            ui_enabled,
            in_game: false,
        }
    }

    /// Whether a loader should open an output device for `set_sounds`.
    pub fn needs_output(&self) -> bool {
        self.output.is_none()
    }

    /// Takes over freshly loaded sounds (and the device, if one came).
    pub fn set_sounds(&mut self, sounds: MenuSounds, output: Option<MixerDeviceSink>) {
        if self.output.is_none() {
            self.output = output;
        }
        self.stop_music();
        self.hover = sounds.hover.and_then(Cue::new);
        self.click = sounds.click.and_then(Cue::new);
        let Some(output) = &self.output else {
            return;
        };
        let Some(clip) = sounds.music else {
            return;
        };
        let (Some(channels), Some(rate)) = (NonZero::new(clip.channels), NonZero::new(clip.rate))
        else {
            return;
        };
        let data: Arc<[rodio::Sample]> = clip
            .samples
            .into_iter()
            .map(|s| s as rodio::Sample)
            .collect();
        let ramp = Arc::new(RampControl::default());
        let player = Player::connect_new(output.mixer());
        player.set_volume(clip.volume);
        player.set_speed(clip.pitch);
        player.pause();
        player.append(GainRamp::new(
            Looping::new(channels, rate, data),
            ramp.clone(),
        ));
        self.music = Some(Music { player, ramp });
        self.apply_music();
    }

    /// Starts the fade the current state asks for: in over 1.5 s when the
    /// music should be heard, out over 0.8 s otherwise.
    fn apply_music(&self) {
        let Some(music) = &self.music else {
            return;
        };
        if self.music_enabled && !self.in_game {
            music.player.play();
            music.ramp.fade_to(1.0, FADE_IN);
        } else {
            music.ramp.fade_to(0.0, FADE_OUT);
        }
    }

    /// Pauses the music player once a fade-out has finished. Optional: a
    /// faded-out player is already silent, this only saves mixing work.
    pub fn tick(&self) {
        if let Some(music) = &self.music
            && music.ramp.target() == 0.0
            && music.ramp.current() == 0.0
        {
            music.player.pause();
        }
    }

    pub fn stop_music(&mut self) {
        self.music = None;
    }

    fn one_shot(&self, cue: Option<&Cue>) -> Option<Player> {
        let output = self.output.as_ref()?;
        let cue = cue?;
        let player = Player::connect_new(output.mixer());
        player.set_volume(cue.volume);
        player.set_speed(cue.pitch);
        player.append(cue.buffer.clone());
        Some(player)
    }

    pub fn hover(&mut self) {
        if !self.ui_enabled {
            return;
        }
        // Dropping the previous player stops its sound without blocking.
        self.hover_player = self.one_shot(self.hover.as_ref());
    }

    pub fn click(&mut self) {
        if !self.ui_enabled {
            return;
        }
        self.click_player = self.one_shot(self.click.as_ref());
    }

    pub fn set_music_enabled(&mut self, on: bool) {
        self.music_enabled = on;
        self.apply_music();
    }

    pub fn set_ui_enabled(&mut self, on: bool) {
        self.ui_enabled = on;
        if !on {
            self.hover_player = None;
            self.click_player = None;
        }
    }

    pub fn game_started(&mut self) {
        self.in_game = true;
        self.apply_music();
    }

    pub fn game_stopped(&mut self) {
        self.in_game = false;
        self.apply_music();
    }
}

/// Records `now` as the latest value and says whether it is new: a change
/// to some other `Some` value. Going to `None` resets without a sound.
pub fn changed<T: PartialEq + Copy>(last: &mut Option<T>, now: Option<T>) -> bool {
    let new = now.is_some() && now != *last;
    *last = now;
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mono, 1000 Hz, all ones: the ramp's output is its gain.
    fn ones(control: &Arc<RampControl>) -> GainRamp<Looping> {
        let data: Arc<[rodio::Sample]> = vec![1.0; 10].into();
        let one = NonZero::new(1).unwrap();
        let rate = NonZero::new(1000).unwrap();
        GainRamp::new(Looping::new(one, rate, data), control.clone())
    }

    fn pull(source: &mut impl Iterator<Item = rodio::Sample>, n: usize) -> rodio::Sample {
        source.by_ref().take(n).last().unwrap()
    }

    #[test]
    fn step_gain_moves_towards_target_without_overshoot() {
        assert_eq!(step_gain(0.0, 1.0, 0.25), 0.25);
        assert_eq!(step_gain(0.9, 1.0, 0.25), 1.0);
        assert_eq!(step_gain(0.5, 0.0, 0.25), 0.25);
        assert_eq!(step_gain(0.1, 0.0, 0.25), 0.0);
        assert_eq!(step_gain(0.4, 0.4, 0.25), 0.4);
    }

    #[test]
    fn gain_ramp_fades_in_out_and_reverses_mid_fade() {
        let control = Arc::new(RampControl::default());
        let mut source = ones(&control);
        assert_eq!(pull(&mut source, 10), 0.0);
        control.fade_to(1.0, 1.5);
        assert!((pull(&mut source, 750) - 0.5).abs() < 0.01);
        assert!((pull(&mut source, 750) - 1.0).abs() < 0.01);
        assert_eq!(pull(&mut source, 10), 1.0);
        assert_eq!(control.current(), 1.0);
        // Game starts: out over 0.8 s.
        control.fade_to(0.0, 0.8);
        assert!((pull(&mut source, 400) - 0.5).abs() < 0.01);
        // Game exits halfway: back in from 0.5, at 0.5 per 1.5 s.
        control.fade_to(1.0, 1.5);
        assert!((pull(&mut source, 750) - 0.75).abs() < 0.01);
        assert!((pull(&mut source, 750) - 1.0).abs() < 0.01);
        control.fade_to(0.0, 0.8);
        assert_eq!(pull(&mut source, 900), 0.0);
        assert_eq!(control.current(), 0.0);
    }

    /// A fade asked for when the gain is a hair off its target still
    /// lands exactly on it within the fade time.
    #[test]
    fn fade_set_at_its_target_still_finishes() {
        let control = Arc::new(RampControl::default());
        control.current.store(1e-6_f32.to_bits(), Ordering::Release);
        let mut source = ones(&control);
        control.fade_to(0.0, 0.8);
        assert!(control.rate() > 0.001);
        pull(&mut source, 800);
        assert_eq!(control.current(), 0.0);
        // Exactly at the target: a positive rate, and nothing moves.
        control.fade_to(0.0, 0.8);
        assert!(control.rate() > 0.0);
        assert_eq!(pull(&mut source, 10), 0.0);
    }

    #[test]
    fn gain_ramp_steps_once_per_stereo_frame() {
        let control = Arc::new(RampControl::default());
        let data: Arc<[rodio::Sample]> = vec![1.0; 8].into();
        let two = NonZero::new(2).unwrap();
        let rate = NonZero::new(100).unwrap();
        let mut source = GainRamp::new(Looping::new(two, rate, data), control.clone());
        control.fade_to(1.0, 1.0);
        let first: Vec<_> = source.by_ref().take(4).collect();
        assert_eq!(first, vec![0.01, 0.01, 0.02, 0.02]);
    }

    #[test]
    fn looping_wraps_and_empty_ends() {
        let one = NonZero::new(1).unwrap();
        let rate = NonZero::new(10).unwrap();
        let data: Arc<[rodio::Sample]> = vec![1.0, 2.0, 3.0].into();
        let got: Vec<_> = Looping::new(one, rate, data).take(7).collect();
        assert_eq!(got, vec![1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 1.0]);
        let empty: Arc<[rodio::Sample]> = Vec::new().into();
        assert_eq!(Looping::new(one, rate, empty).next(), None);
    }

    #[test]
    fn changed_fires_once_per_new_value() {
        let mut last = None;
        assert!(changed(&mut last, Some(1)));
        assert!(!changed(&mut last, Some(1)));
        assert!(changed(&mut last, Some(2)));
        assert!(!changed(&mut last, None));
        assert!(changed(&mut last, Some(2)));
    }

    /// Without a device everything is a silent no-op.
    #[test]
    fn silent_audio_without_output() {
        let mut audio = MenuAudio::new(true, true);
        audio.set_sounds(MenuSounds::default(), None);
        audio.game_started();
        audio.tick();
        audio.hover();
        audio.click();
        audio.game_stopped();
        assert!(audio.output.is_none() && audio.music.is_none());
        assert!(audio.hover_player.is_none() && audio.click_player.is_none());
    }

    #[test]
    fn output_device_can_come_from_the_loader_thread() {
        fn send<T: Send>() {}
        send::<Loaded>();
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.002
    }

    /// Reads the real sounds: `MASHUP_MW2=<mw2 folder> cargo test -p
    /// mashup_launcher -- --ignored menu_audio`.
    #[test]
    #[ignore = "needs an MW2 install in MASHUP_MW2"]
    fn menu_audio_loads_real_mw2_sounds() {
        let mw2 = PathBuf::from(std::env::var("MASHUP_MW2").expect("MASHUP_MW2"));
        let started = std::time::Instant::now();
        let sounds = load(&mw2).unwrap();
        let elapsed = started.elapsed();
        let music = sounds.music.expect("music");
        let hover = sounds.hover.expect("hover");
        let click = sounds.click.expect("click");
        for (name, clip) in [("music", &music), ("hover", &hover), ("click", &click)] {
            eprintln!(
                "{name}: {} ch, {} Hz, {:.2} s, volume {:.3}, pitch {:.2}",
                clip.channels,
                clip.rate,
                clip.seconds(),
                clip.volume,
                clip.pitch
            );
        }
        eprintln!("loaded in {elapsed:?}");
        assert_eq!((music.channels, music.rate), (2, 44_100));
        // hz_t_oilrig_themestealth_v1.mp3 is 1:52.82 long.
        assert!((105.0..=120.0).contains(&music.seconds()));
        assert!(approx(music.volume, 0.324));
        assert!(approx(hover.volume, 0.271));
        assert!(approx(hover.pitch, 1.3));
        assert!(approx(click.volume, 0.211));
        assert!(approx(click.pitch, 1.0));
        assert!(hover.seconds() > 0.0 && click.seconds() > 0.0);
    }
}
