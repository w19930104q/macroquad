//! Fixed sample MQ-C: six deterministic sounds, four in the `music` group
//! and two in the `ui` group, exercising group volume, group stop and
//! pause/resume in three steps.

use macroquad::audio::*;

/// A fixed 16-bit PCM mono WAV tone, generated in code so the sample set is
/// identical on every run and needs no asset files.
fn fixed_tone(freq: f32) -> Vec<u8> {
    const RATE: u32 = 44100;
    let frames = RATE as usize / 10; // 0.1 s
    let data_size = (frames * 2) as u32;

    let mut wav = Vec::with_capacity(44 + data_size as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for i in 0..frames {
        let t = i as f32 / RATE as f32;
        let sample = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.5;
        wav.extend_from_slice(&((sample * i16::MAX as f32) as i16).to_le_bytes());
    }
    wav
}

/// The six fixed MQ-C sounds: four music tones and two ui tones.
async fn load_mq_c_sounds() -> (Vec<Sound>, Vec<Sound>) {
    let mut music = Vec::new();
    for freq in [220.0, 261.63, 329.63, 440.0] {
        music.push(load_sound_from_bytes(&fixed_tone(freq)).await.unwrap());
    }
    let mut ui = Vec::new();
    for freq in [660.0, 880.0] {
        ui.push(load_sound_from_bytes(&fixed_tone(freq)).await.unwrap());
    }
    (music, ui)
}

fn play_all_looped(music: &[Sound], ui: &[Sound]) {
    for sound in music {
        play_sound_in_group(
            sound,
            SoundGroup::MUSIC,
            PlaySoundParams {
                looped: true,
                volume: 1.0,
            },
        );
    }
    for sound in ui {
        play_sound_in_group(
            sound,
            SoundGroup::UI,
            PlaySoundParams {
                looped: true,
                volume: 1.0,
            },
        );
    }
}

#[macroquad::test]
async fn mq_c_audio_groups() {
    let (music, ui) = load_mq_c_sounds().await;
    assert_eq!(music.len(), 4);
    assert_eq!(ui.len(), 2);
    play_all_looped(&music, &ui);

    // ---- step 1: group volume ----
    // Dropping music by 6 dB must attenuate each of the four music sounds
    // to 0.501 while the two ui sounds stay at 1.0.
    set_group_volume_db(SoundGroup::MUSIC, -6.0);
    let music_gain = 10f32.powf(-6.0 / 20.0);
    assert!(
        (music_gain - 0.501).abs() < 1e-3,
        "MQ-C reference: -6 dB == 0.501 linear, got {music_gain}"
    );
    assert!((group_volume(SoundGroup::MUSIC) - music_gain).abs() < 1e-6);
    assert_eq!(group_volume(SoundGroup::UI), 1.0);
    for sound in &music {
        let effective = sound_effective_volume(sound);
        assert!(
            (effective - 0.501).abs() < 1e-3,
            "music sound attenuated to 0.501, got {effective}"
        );
    }
    for sound in &ui {
        assert_eq!(sound_effective_volume(sound), 1.0, "ui stays at 1.0");
    }

    // The settings page master slider affects every group.
    set_master_volume(0.5);
    for sound in &music {
        assert!((sound_effective_volume(sound) - music_gain * 0.5).abs() < 1e-6);
    }
    for sound in &ui {
        assert!((sound_effective_volume(sound) - 0.5).abs() < 1e-6);
    }
    set_master_volume(1.0);

    // Scene switch: every sound handle is dropped and the scene is rebuilt
    // from scratch. Group volumes live in the audio context, not in the
    // sounds, so the music attenuation must still be there.
    drop(music);
    drop(ui);
    let (music, ui) = load_mq_c_sounds().await;
    play_all_looped(&music, &ui);
    assert!((group_volume(SoundGroup::MUSIC) - music_gain).abs() < 1e-6);
    for sound in &music {
        assert!((sound_effective_volume(sound) - 0.501).abs() < 1e-3);
    }
    for sound in &ui {
        assert_eq!(sound_effective_volume(sound), 1.0);
    }

    // ---- step 2: stopping the music group leaves ui playing ----
    stop_group(SoundGroup::MUSIC);
    for sound in &music {
        assert!(!is_sound_playing(sound), "music group is stopped");
    }
    for sound in &ui {
        assert!(is_sound_playing(sound), "ui keeps playing");
    }

    // ---- step 3: pause/resume keeps group settings ----
    // The legacy per-sound volume interface still works alongside groups.
    set_sound_volume(&ui[0], 0.5);
    assert!((sound_volume(&ui[0]) - 0.5).abs() < 1e-6);
    assert!((sound_effective_volume(&ui[0]) - 0.5).abs() < 1e-6);

    pause_audio();
    assert!(audio_paused());
    assert_eq!(sound_effective_volume(&ui[0]), 0.0, "paused output is muted");
    assert!(
        (group_volume(SoundGroup::MUSIC) - music_gain).abs() < 1e-6,
        "group volumes survive pausing"
    );
    assert_eq!(group_volume(SoundGroup::UI), 1.0);
    assert_eq!(master_volume(), 1.0);

    resume_audio();
    assert!(!audio_paused());
    assert!(
        (group_volume(SoundGroup::MUSIC) - music_gain).abs() < 1e-6,
        "group volumes survive resume"
    );
    assert!(
        (sound_effective_volume(&ui[0]) - 0.5).abs() < 1e-6,
        "per-sound volume restored after resume"
    );
    assert_eq!(sound_effective_volume(&ui[1]), 1.0);

    stop_group(SoundGroup::UI);
    for sound in &ui {
        assert!(!is_sound_playing(sound));
    }
}
