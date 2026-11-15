# Final Night 7: itch.io page text

## Tagline

Type faster than the gods: your words become history.

## Description

Final Night 7 is a multiplayer god game played on the real Earth. Each player is a god with two demigods, a Left Hand
and a Right Hand, and you command them by writing. Start typing and time slows for everyone; the first god to press
Enter cuts the others off. Throw mega powers across the globe or mini powers onto the board, and watch a living voxel
earth flood, burn, quake and rise, hardest along real fault lines. An AI Dungeon Master reads each place's history,
rolls one d20 and explains the result. Meet a rival and duel by typing, using only what you hold or find. Every match
is recorded: replay it exactly, fork any moment, and watch your storyboard, the final cut. Last one standing wins.

## How to play

- **Join** the house game from the lobby (or host your own room: **TO CONFIRM** the lobby options in the build).
- **Move the world:** drag the globe to spin it, drag the board to pan, double-click to zoom in, right-click to zoom
  out.
- **Your will:** click YOUR WILL and write what your Left or Right Hand does. Time slows for every player while you
  write. Press Enter to send. The first Enter closes the window, and Time bills you a bolt, a debt and some Entropy.
- **Powers:** you hold lightning bolts (mega) and sparks (mini). Drop a mega on the globe for a map-wide strike, or a
  mini on the board for a local one. Every power raises Entropy; at 100 the world ends in heat death.
- **Summon:** drag a demigod's card onto the board or the globe to send it there.
- **Duels:** double-click a rival in reach to see your odds and challenge. Each line you type is a blow, played on
  Enter ("roll left", "jump", "throw dirt in his eyes"). Only what you hold, or what's on the board, works.
- **Win:** be the last one standing when the Final Night falls.

## AI tools used

- **Claude** (Anthropic, `claude-haiku-5-5`), on the server: the game master, the gods, the narrator, the duel move
  picker and the god-form moderation. With no AI, every mind falls back to rules.
- **OpenAI `gpt-image-1-mini`**, on the server: demigod portraits and dioramas, event moment cards, the end-of-match
  storyboard, people and building pictures, and god-form previews.
- **Meshy**: the marble god statues, the heroic demigod statues, players' 3D god forms, and the duel mannequin with its
  skins, rig and fight clips.
- **Fish Audio** (Drama 3, designed voices; Chris's TTS pipeline): the voiceovers (welcome, intro, clash, victory,
  defeat, duel won, duel lost). **TO CONFIRM** they are in the shipped build.
- **Claude Code** with Opus 5.5 agents: development help.

No AI key ships in the game; all AI work runs on the server.

## Prior work declared

Made before the jam (Nov 12, 2026) and declared:

- The 2024 Final Night prototype (Node backend and Unity client).
- Final Night 2.5, the earlier plugin for The Theatre engine.
- Pre-jam prompts, scripts and agent setup.
- **TO CONFIRM:** which parts of The Theatre engine existed before the jam.

The gameplay of Final Night 7 was built during the jam. Dev log: `BUILDLOG.md` (the designers' words, recorded as they
were given, with engineering notes).

## Third-party assets

- **Plate boundaries** (the in-game fault lines): "Plate boundaries: Bird, P. (2002), PB2002; GeoJSON by Hugo
  Ahlenius, Nordpil (ODC-BY 1.0)." Citation: Bird, P. (2002), An updated digital model of plate boundaries, G3 4(3),
  1027.
- **Natural Earth** coastlines and cities (public domain).
- **Unreal Engine** basic shapes.
- **Meshy**-generated meshes, textures and animations (see "AI tools used").
- **Fish Audio** voiceovers: Fish Audio Community License; designed voices, no real person's voice.
- **TO CONFIRM:** any Fab, music, sound-effect or font assets in the build (none are listed in the docs), and Benito's
  reference images (e.g. the star backdrop reference).

## Team

- **Chris**: design and direction.
- **Benito**: the Unreal client, visual style, duel and event design.

**TO CONFIRM:** team name and final credits.
