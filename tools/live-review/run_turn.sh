#!/usr/bin/env bash
# One live lip-sync check against the staged review app and practice game.
#
#   tools/live-review/run_turn.sh <out-dir> [--relaunch]
#
# Drives the app over WebView2 DevTools (cdp.py), keeps both windows on the
# area given by GAME_RECT/APP_RECT (physical pixels; default: the right edge of
# a second monitor at x 2560..6000), then records the game's on-screen pixels,
# the speaker loopback, and the app's own visual presentation receipts.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$1"; shift || true
RELAUNCH=0
[[ "${1:-}" == "--relaunch" ]] && RELAUNCH=1
REVIEW_ROOT="${REVIEW_ROOT:-E:\\work bench\\Interactive LLM\\review}"
GAME_RECT="${GAME_RECT:-4980,760,976,639}"
APP_RECT="${APP_RECT:-2600,40,1100,800}"
SECONDS_TO_RECORD="${SECONDS_TO_RECORD:-16}"
# Client-relative area around the practice game's face, sampled at ~30 fps during
# the turn. Baseline frames keep the full client area for compositing residuals.
MOUTH_CROP="${MOUTH_CROP:-380,200,300,220}"
mkdir -p "$OUT"
cd "$HERE"

if (( RELAUNCH )); then
  # Debug worker builds write every proposed mouth residual here. The broker's
  # overlay is excluded from screen capture, so these dumps (composited by
  # composite_residuals.py) are the only way to see the presented mouth.
  export NPC_MOUTH_WORKER_DEBUG_RESIDUAL_DIR="$(cygpath -w "$OUT")\\residuals"
  # Debug app builds append per-frame visual pipeline stage times here.
  export NPC_DEBUG_VISUAL_TIMING_LOG="$(cygpath -w "$OUT")\\visual-timing.log"
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File relaunch.ps1 \
    -ReviewRoot "$REVIEW_ROOT" -Rect "$APP_RECT" >/dev/null
  for _ in $(seq 1 60); do python cdp.py "document.readyState" >/dev/null 2>&1 && break; sleep 0.5; done
  sleep 3
fi

# Launch (or re-bind) the practice game, then park it on the second screen.
( for _ in $(seq 1 150); do
    python place_window.py --process interactive-npcs-synthetic-target.exe --rect "$GAME_RECT" --top \
      >/dev/null 2>&1 && break
    sleep 0.1
  done ) &
python cdp.py --timeout 40 "window.__TAURI_INTERNALS__.invoke('prepare_synthetic_review_target')
  .then(r=>'game pid '+r.targetProcessId+(r.launched?' (launched)':' (already bound)'),e=>'connect failed: '+JSON.stringify(e))"
wait
# Other windows on the same screen can sit above a background-raised window, so
# keep the game topmost while recording and drop that again on exit.
python place_window.py --process interactive-npcs-synthetic-target.exe --rect "$GAME_RECT" --topmost >/dev/null
trap 'python place_window.py --process interactive-npcs-synthetic-target.exe --rect "$GAME_RECT" --no-topmost >/dev/null 2>&1 || true' EXIT

python cdp.py "(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='Use a typed setup turn instead'); if(b) b.click(); return 'typed setup '+(b?'selected':'already selected')})()"
sleep 1
before=$(python cdp.py "window.__TAURI_INTERNALS__.invoke('debug_visual_presentation_receipt_history').then(r=>r.length)")

python capture_window.py --process interactive-npcs-synthetic-target.exe --out "$OUT" --prefix base --screen \
  --count 10 --interval-ms 100 >/dev/null
python record_loopback.py --seconds "$SECONDS_TO_RECORD" --out "$OUT/turn.wav" >"$OUT/audio.txt" 2>/dev/null &
python capture_window.py --process interactive-npcs-synthetic-target.exe --out "$OUT" --prefix turn --screen \
  --crop "$MOUTH_CROP" --count $(( SECONDS_TO_RECORD * 1000 / 33 )) --interval-ms 33 >"$OUT/frames.txt" 2>&1 &
sleep 0.5
python cdp.py "(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='Run bounded typed turn'); if(!b) return 'run button missing'; if(b.disabled) return 'run button disabled'; b.click(); return 'turn started'})()"
wait

python cdp.py "window.__TAURI_INTERNALS__.invoke('debug_visual_presentation_receipt_history')
  .then(r=>r.slice($before),e=>({err:e}))" >"$OUT/receipts.json"
python - "$OUT" <<'EOF'
import collections, json, pathlib, sys
out = pathlib.Path(sys.argv[1])
receipts = json.loads((out / "receipts.json").read_text(encoding="utf-8"))
if isinstance(receipts, dict):
    sys.exit(f"receipt query failed: {receipts}")
presented = sum(1 for r in receipts if r.get("presented"))
print(f"receipts this turn: {len(receipts)}  presented: {presented}")
counts = collections.Counter((r.get("diagnosticErrorCode") or "-", r["detail"][:100]) for r in receipts)
for (code, detail), n in counts.most_common(12):
    print(f"  {n:4d}  {code:38s} {detail}")
EOF
sed -n "2,3p" "$OUT/audio.txt"
if [[ -d "$OUT/residuals" ]]; then
  python composite_residuals.py --dir "$(cygpath -w "$OUT")\residuals" \
    --sheet "$(cygpath -w "$OUT")\mouth-sheet.png" >"$OUT/residuals.txt" && tail -2 "$OUT/residuals.txt"
fi
