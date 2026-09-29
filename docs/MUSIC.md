# `docs/` — the one-page map

The music player and the Skate 3 music extraction.

## The player

Plays whatever the user drops into `music/` **beside the executable**
(`IW4L_MUSIC` overrides the folder). It scans at startup and on `music reload`,
plays one file at a time, and advances when a track ends. Its entities carry no
match `Voice`, so playback survives map loads.

```
music                       status (folder, count, current, volume, shuffle, repeat)
music on | off | toggle
music next | prev
music list                  numbered tracks, current marked
music volume <0-1>
music shuffle [on|off]
music repeat <off|all|one>
music track <n>
music reload                rescan the folder
```

Formats: `.mp3`, `.wav`, `.ogg`, `.flac` (symphonia). Each decoded track is
peak-normalised to `0.95` before it enters the mixer — music is usually mastered
far below game effects — then scaled by `music volume` (default `1.0`).
Implementation: `crates/audio/src/music.rs`, command in
`crates/console/src/music.rs`.

### Game sound vs music

Game sound is a separate gain from music, so you can drop the effects and keep
the music:

```
sound            print the game-sound volume
sound <0-1>      set it (same setting the options menu's volume slider writes)
music volume <0-1>
```

`audio::backend` applies the game gain to **every** voice it starts (the old
code only reached positional and ambient sounds, so the slider barely did
anything) and rescales live loops when it changes. Music is deliberately
excluded — the two are independent.

### On-screen player

A small panel in the top-right of the HUD. `F8` hides/shows it; `music ui
on|off` does the same from the console.

| key | action |
|---|---|
| `[` / `]` | previous / next track |
| `\` | play / pause |
| `-` / `=` | music volume down / up |
| `;` / `'` | game sound volume down / up |
| `F8` | hide / show the panel |

Implementation: `crates/console/src/music_hud.rs`.

## Getting the Skate 3 music out of the disc

The disc's `data/audio/music/` holds three EA **EAAC / SNR+SNS** banks:

| bank | `.mpf` | sounds | what it is |
|---|---|---|---|
| iPod | `ipod.mpf` | 1380 | the in-game iPod/party soundtrack |
| World | `world.mpf` | 5725 | the Free Skate / open-world bed |
| Game | `game.mpf` | 1074 | adaptive event/scoring music |

These are **not song files**. The `.mpf` defines only 1–2 interactive "tracks"
made of thousands of short sound segments (roughly 2–10 s each); there are no
per-song names or boundaries anywhere in the format, so individual licensed
songs cannot be split out. What the banks *do* give is one continuous mix per
bank, which is what the community extractors produce too.

`ffmpeg` cannot read `.mus`; use `vgmstream` (`vgmstream-cli -i -S 0`, which
opens a `.mpf` and its paired `_Stream.mus`, `-i` to ignore loop points). The
copy used here lives at `C:\Games\tools\vgmstream`.

The current `music/` content was produced by exporting every subsong, letting
`ffmpeg` concat them into one continuous MP3 per bank, then splitting that into
four-minute chunks (`-c copy`) so the game never buffers a whole multi-hour
file as `f32`:

```
target/play/music/Skate 3 - iPod 00..36.mp3
target/play/music/Skate 3 - Free Skate 00..NN.mp3
```
