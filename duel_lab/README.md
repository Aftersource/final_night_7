# Duel Lab

A browser (three.js) playback of one duel, used to tune the challenge box before the Unreal client builds it.

Run: `cd duel_lab && python3 -m http.server 8177`, then open http://localhost:8177/ .
Keys: space = play/pause, R = replay, left/right = step 0.1 s. The slider scrubs; everything on screen is a pure
function of the timeline time, so rewind and replay are exact. URL: `?t=<ms>&paused=1&a=<skin>&b=<skin>&duel=<file>`.

- `duel.json`: the duel script (the data model). `set[]` is the stage dressing in basic shapes on hex coordinates.
  Each panel has `play_ms` (how long it plays), `input_ms` (when the players' typing locks, always shorter), a caption,
  a camera `shot`, per-fighter `segs[]` (`clip`, `ms`, `to` hex, `path` dash/leap/flip/knock, `h`, `speed`, `at`),
  `contacts[]` (hit time and burst word), `flip` (initiative) and `hp` after the panel.
- `assets/mannequin.glb`: the Meshy figure-drawing mannequin, rigged to the UE5 mannequin bone names.
- `assets/anims/*.glb`: 35 clips from Meshy's library, stripped to skeleton + animation, named by our pose names.
- `assets/skins/*`: wood, obsidian, gold, dragonscale, butterfly (PBR maps on the same UVs).
- `tools/`: `fetch_anims.sh` + `convert.mjs` (Meshy GLB to web assets), `shot.mjs` (headless screenshots into `shots/`).
