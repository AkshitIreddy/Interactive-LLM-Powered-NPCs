# Live review tools

Local, developer-only helpers for checking a debug review build end to end
against the synthetic practice game, without driving the mouse or keyboard.

```
powershell -File tools/live-review/stage-review.ps1 -Destination <review dir> -TestGame <local-app-data\test-game> [-MouthAtlas <pack dir>]
bash tools/live-review/run_turn.sh <out dir> --relaunch
```

`run_turn.sh` relaunches the staged app with WebView2 remote debugging, binds the
practice game through the app's own commands (`cdp.py`), parks both windows on a
second monitor (`GAME_RECT`/`APP_RECT`, physical pixels), runs one bounded typed
test turn, and collects:

- `receipts.json`: the app's visual presentation receipts for the turn;
- `turn-*.png`: ~30 fps grabs of the face area (`capture_window.py --screen`);
- `turn.wav` / `audio.txt`: a speaker loopback with a testing-tone band check;
- `residuals/` and `mouth-sheet.png`: every mouth residual the worker proposed,
  composited over its source frame (`composite_residuals.py`);
- `visual-timing.log`: per-frame visual pipeline stage times.

The broker's mouth overlay is excluded from screen capture by design, so screen
grabs never show the presented mouth. The residual dumps
(`NPC_MOUTH_WORKER_DEBUG_RESIDUAL_DIR`) and stage timing
(`NPC_DEBUG_VISUAL_TIMING_LOG`) are only honoured by debug app builds.

Display safety: these tools open only the app and the practice game, whose
static mode changes a small dark patch by one shade per frame. Never use them
to launch the native smoke-test windows.
