//! Loading and playing sounds.
//!
//! Sounds may be assigned to [`SoundGroup`]s (like music or UI sounds) so a
//! settings page can control a whole category at once. Group and master
//! volumes live in the global audio context, so they survive scene switches:
//! sounds loaded after a group volume was set still play with that group's
//! volume applied.

#![allow(dead_code)]

use crate::{file::load_file, get_context, Error};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

#[cfg(feature = "audio")]
use quad_snd::{AudioContext as QuadSndContext, Sound as QuadSndSound};

#[cfg(feature = "audio")]
pub use quad_snd::PlaySoundParams;

#[cfg(not(feature = "audio"))]
mod dummy_audio {
    use crate::audio::PlaySoundParams;

    pub struct AudioContext {}

    impl AudioContext {
        pub fn new() -> AudioContext {
            AudioContext {}
        }

        #[cfg(target_os = "android")]
        pub fn pause(&mut self) {}

        #[cfg(target_os = "android")]
        pub fn resume(&mut self) {}
    }

    pub struct Sound {}

    impl Sound {
        pub fn load(_ctx: &mut AudioContext, _data: &[u8]) -> Sound {
            Sound {}
        }

        pub fn play(&self, _ctx: &mut AudioContext, _params: PlaySoundParams) {
            eprintln!("warn: macroquad's \"audio\" feature disabled.");
        }

        pub fn stop(&self, _ctx: &mut AudioContext) {}

        pub fn set_volume(&self, _ctx: &mut AudioContext, _volume: f32) {}

        #[allow(dead_code)]
        pub fn is_loaded(&self) -> bool {
            true
        }

        pub fn delete(&self, _ctx: &AudioContext) {}
    }
}

#[cfg(not(feature = "audio"))]
use dummy_audio::{AudioContext as QuadSndContext, Sound as QuadSndSound};

#[cfg(not(feature = "audio"))]
pub struct PlaySoundParams {
    pub looped: bool,
    pub volume: f32,
}

/// A named collection of sounds whose volume is controlled together.
///
/// Groups are identified by a `u32`, so games can define their own groups in
/// addition to the built-in [`SoundGroup::MUSIC`] and [`SoundGroup::UI`].
/// Group volumes are stored in the global audio context, not in individual
/// sounds, which keeps them intact across scene switches.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SoundGroup(pub u32);

impl SoundGroup {
    /// The group every sound starts in.
    pub const DEFAULT: SoundGroup = SoundGroup(0);
    /// Background music.
    pub const MUSIC: SoundGroup = SoundGroup(1);
    /// UI feedback sounds and notification beeps.
    pub const UI: SoundGroup = SoundGroup(2);
}

/// Per-sound bookkeeping kept in the audio context.
struct SoundEntry {
    group: SoundGroup,
    /// Volume set through `play_sound` params or `set_sound_volume`,
    /// before group/master multipliers.
    volume: f32,
    active_playbacks: u32,
    sound: Weak<QuadSndSoundGuarded>,
}

pub struct AudioContext {
    native_ctx: QuadSndContext,
    master_volume: f32,
    paused: bool,
    group_volumes: HashMap<SoundGroup, f32>,
    sounds: HashMap<u64, SoundEntry>,
}

/// Final volume pushed to the native backend for a sound.
fn mix_volume(paused: bool, master_volume: f32, group_volume: f32, volume: f32) -> f32 {
    if paused {
        0.0
    } else {
        volume * group_volume * master_volume
    }
}

impl AudioContext {
    pub fn new() -> AudioContext {
        AudioContext {
            native_ctx: QuadSndContext::new(),
            master_volume: 1.0,
            paused: false,
            group_volumes: HashMap::new(),
            sounds: HashMap::new(),
        }
    }

    fn group_volume(&self, group: SoundGroup) -> f32 {
        self.group_volumes.get(&group).copied().unwrap_or(1.0)
    }

    fn effective_volume(&self, entry: &SoundEntry) -> f32 {
        mix_volume(
            self.paused,
            self.master_volume,
            self.group_volume(entry.group),
            entry.volume,
        )
    }

    /// Recompute the effective volume of one sound and push it to the backend.
    fn apply_effective_volume(&mut self, id: u64) {
        let (volume, sound) = match self.sounds.get(&id) {
            Some(entry) => (self.effective_volume(entry), entry.sound.upgrade()),
            None => return,
        };
        if let Some(sound) = sound {
            sound.sound.set_volume(&mut self.native_ctx, volume);
        }
    }

    fn apply_all_volumes(&mut self) {
        let ids: Vec<u64> = self.sounds.keys().copied().collect();
        for id in ids {
            self.apply_effective_volume(id);
        }
    }

    /// Pause all audio output. Group and master volumes are kept and
    /// re-applied on [`AudioContext::resume`].
    pub fn pause(&mut self) {
        if self.paused {
            return;
        }
        self.paused = true;
        #[cfg(target_os = "android")]
        self.native_ctx.pause();
        self.apply_all_volumes();
    }

    /// Resume audio output after [`AudioContext::pause`], restoring the
    /// exact group/master volumes that were in effect before pausing.
    pub fn resume(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        #[cfg(target_os = "android")]
        self.native_ctx.resume();
        self.apply_all_volumes();
    }
}

static NEXT_SOUND_ID: AtomicU64 = AtomicU64::new(0);

struct QuadSndSoundGuarded {
    id: u64,
    sound: QuadSndSound,
}

impl Drop for QuadSndSoundGuarded {
    fn drop(&mut self) {
        let ctx = &mut get_context().audio_context;
        self.sound.delete(&ctx.native_ctx);
        ctx.sounds.remove(&self.id);
    }
}

#[derive(Clone)]
pub struct Sound(Arc<QuadSndSoundGuarded>);

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sound").finish()
    }
}

/// Load audio file.
///
/// Attempts to automatically detect the format of the source of data.

pub async fn load_sound(path: &str) -> Result<Sound, Error> {
    let data = load_file(path).await?;

    load_sound_from_bytes(&data).await
}

/// Load audio data.
///
/// Attempts to automatically detect the format of the source of data.
pub async fn load_sound_from_bytes(data: &[u8]) -> Result<Sound, Error> {
    let sound = {
        let ctx = &mut get_context().audio_context;
        QuadSndSound::load(&mut ctx.native_ctx, data)
    };

    // only on wasm the sound is not ready right away
    #[cfg(target_arch = "wasm32")]
    while sound.is_loaded() == false {
        crate::window::next_frame().await;
    }

    let id = NEXT_SOUND_ID.fetch_add(1, Ordering::Relaxed);
    let guarded = Arc::new(QuadSndSoundGuarded { id, sound });
    get_context().audio_context.sounds.insert(
        id,
        SoundEntry {
            group: SoundGroup::DEFAULT,
            volume: 1.0,
            active_playbacks: 0,
            sound: Arc::downgrade(&guarded),
        },
    );

    Ok(Sound(guarded))
}

pub fn play_sound_once(sound: &Sound) {
    play_sound(
        sound,
        PlaySoundParams {
            looped: false,
            volume: 1.0,
        },
    );
}

pub fn play_sound(sound: &Sound, params: PlaySoundParams) {
    let ctx = &mut get_context().audio_context;
    let id = sound.0.id;

    let mut group = SoundGroup::DEFAULT;
    if let Some(entry) = ctx.sounds.get_mut(&id) {
        entry.volume = params.volume;
        entry.active_playbacks += 1;
        group = entry.group;
    }

    let volume = mix_volume(
        ctx.paused,
        ctx.master_volume,
        ctx.group_volume(group),
        params.volume,
    );
    sound.0.sound.play(
        &mut ctx.native_ctx,
        PlaySoundParams {
            looped: params.looped,
            volume,
        },
    );
}

/// Assign a sound to a group and play it.
///
/// Equivalent to [`set_sound_group`] followed by [`play_sound`].
pub fn play_sound_in_group(sound: &Sound, group: SoundGroup, params: PlaySoundParams) {
    set_sound_group(sound, group);
    play_sound(sound, params);
}

pub fn stop_sound(sound: &Sound) {
    let ctx = &mut get_context().audio_context;
    sound.0.sound.stop(&mut ctx.native_ctx);
    if let Some(entry) = ctx.sounds.get_mut(&sound.0.id) {
        entry.active_playbacks = 0;
    }
}

/// Set the volume of a single sound, the same way the legacy API always did.
///
/// The value is multiplied with the sound's group volume and the master
/// volume before reaching the audio backend.
pub fn set_sound_volume(sound: &Sound, volume: f32) {
    let ctx = &mut get_context().audio_context;
    let id = sound.0.id;
    if let Some(entry) = ctx.sounds.get_mut(&id) {
        entry.volume = volume;
    }
    ctx.apply_effective_volume(id);
}

/// The per-sound volume last set through `play_sound` params or
/// [`set_sound_volume`], without group/master multipliers.
pub fn sound_volume(sound: &Sound) -> f32 {
    get_context()
        .audio_context
        .sounds
        .get(&sound.0.id)
        .map(|entry| entry.volume)
        .unwrap_or(1.0)
}

/// The volume actually sent to the audio backend for this sound: per-sound
/// volume x group volume x master volume, or 0 while audio is paused.
pub fn sound_effective_volume(sound: &Sound) -> f32 {
    let ctx = &get_context().audio_context;
    ctx.sounds
        .get(&sound.0.id)
        .map(|entry| ctx.effective_volume(entry))
        .unwrap_or(1.0)
}

/// Whether the sound has playbacks that were started and not stopped since.
///
/// Note: macroquad cannot detect a non-looped playback ending on its own, so
/// such sounds keep reporting `true` until [`stop_sound`] or [`stop_group`].
pub fn is_sound_playing(sound: &Sound) -> bool {
    get_context()
        .audio_context
        .sounds
        .get(&sound.0.id)
        .map(|entry| entry.active_playbacks > 0)
        .unwrap_or(false)
}

/// Move a sound to another group. Currently playing instances of the sound
/// are re-leveled immediately.
pub fn set_sound_group(sound: &Sound, group: SoundGroup) {
    let ctx = &mut get_context().audio_context;
    let id = sound.0.id;
    if let Some(entry) = ctx.sounds.get_mut(&id) {
        entry.group = group;
    }
    ctx.apply_effective_volume(id);
}

pub fn sound_group(sound: &Sound) -> SoundGroup {
    get_context()
        .audio_context
        .sounds
        .get(&sound.0.id)
        .map(|entry| entry.group)
        .unwrap_or(SoundGroup::DEFAULT)
}

/// Set the volume of a whole group of sounds, `1.0` being the default.
///
/// The setting is stored in the global audio context, so it stays in effect
/// across scene switches and applies to sounds loaded afterwards as well.
pub fn set_group_volume(group: SoundGroup, volume: f32) {
    let ctx = &mut get_context().audio_context;
    ctx.group_volumes.insert(group, volume);
    let ids: Vec<u64> = ctx
        .sounds
        .iter()
        .filter(|(_, entry)| entry.group == group)
        .map(|(&id, _)| id)
        .collect();
    for id in ids {
        ctx.apply_effective_volume(id);
    }
}

/// Set a group volume in decibels: `0 dB` is unity gain, `-6 dB` is
/// roughly `0.501` linear, `-inf`-style silence is approached with large
/// negative values.
pub fn set_group_volume_db(group: SoundGroup, db: f32) {
    set_group_volume(group, 10f32.powf(db / 20.0));
}

pub fn group_volume(group: SoundGroup) -> f32 {
    get_context().audio_context.group_volume(group)
}

/// Stop every currently playing sound in the group. Sounds in other groups
/// keep playing.
pub fn stop_group(group: SoundGroup) {
    let ctx = &mut get_context().audio_context;
    let targets: Vec<(u64, Arc<QuadSndSoundGuarded>)> = ctx
        .sounds
        .iter()
        .filter(|(_, entry)| entry.group == group)
        .filter_map(|(&id, entry)| entry.sound.upgrade().map(|sound| (id, sound)))
        .collect();
    for (id, sound) in targets {
        sound.sound.stop(&mut ctx.native_ctx);
        if let Some(entry) = ctx.sounds.get_mut(&id) {
            entry.active_playbacks = 0;
        }
    }
}

/// Set the master volume applied to every sound, `1.0` being the default.
pub fn set_master_volume(volume: f32) {
    let ctx = &mut get_context().audio_context;
    ctx.master_volume = volume;
    ctx.apply_all_volumes();
}

/// Set the master volume in decibels, see [`set_group_volume_db`].
pub fn set_master_volume_db(db: f32) {
    set_master_volume(10f32.powf(db / 20.0));
}

pub fn master_volume() -> f32 {
    get_context().audio_context.master_volume
}

/// Pause all audio output, keeping every group and master volume setting.
pub fn pause_audio() {
    get_context().audio_context.pause();
}

/// Resume audio output after [`pause_audio`]. All group and master volumes
/// are exactly where they were before the pause.
pub fn resume_audio() {
    get_context().audio_context.resume();
}

pub fn audio_paused() -> bool {
    get_context().audio_context.paused
}
