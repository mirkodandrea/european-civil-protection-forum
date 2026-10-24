#!/usr/bin/env python3
"""Compose the standalone demo: screen.mkv + narration + burned-in subtitles in one ffmpeg pass.

  python3 standalone/compose.py <out_dir> <output.mp4> [--fast]

<out_dir> holds screen.mkv, marks.json ({marks, speedups}; times in ms from the first frame),
narration.json (a WAV per line) and subs/*.png (render_subs.js, TTX_SUB_WIDTH=1500). Speed-ups
(waits for the model) are applied to the picture and the marks are remapped, so the voice stays
in step. Writes .srt and .vtt next to the mp4.
"""
import glob, json, os, subprocess, sys, time

out_dir, output = sys.argv[1:3]
fast = "--fast" in sys.argv
rec = json.load(open(os.path.join(out_dir, "marks.json")))
marks, speedups = rec["marks"], sorted(rec["speedups"], key=lambda x: x["from"])
files = {l["id"]: l["file"] for l in json.load(open(os.path.join(out_dir, "narration.json")))["lines"]}
subs = {os.path.basename(p)[3:-4]: p for p in sorted(glob.glob(os.path.join(out_dir, "subs", "[0-9][0-9]_*.png")))}
src = os.path.join(out_dir, "screen.mkv")
dur_src = float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", src]))

# Source segments (start, end, speed factor) and where each lands in the output.
segs, t = [], 0.0
for sp in speedups:
    a, b = sp["from"] / 1000, sp["to"] / 1000
    if a > t:
        segs.append((t, a, 1))
    segs.append((a, b, sp["factor"]))
    t = b
segs.append((t, dur_src, 1))
starts, acc = [], 0.0
for a, b, f in segs:
    starts.append(acc)
    acc += (b - a) / f
duration = acc


def remap(x):  # source seconds -> output seconds
    for (a, b, f), o in zip(segs, starts):
        if a <= x < b:
            return o + (x - a) / f
    return duration


wins = []
for i, m in enumerate(marks):
    a = remap(m["t"] / 1000)
    b = a + m["duration_s"] + 0.3
    if i + 1 < len(marks):
        b = min(b, remap(marks[i + 1]["t"] / 1000))
    wins.append((m, a, min(b, duration)))


def ts(x, sep):
    h, r = divmod(x, 3600)
    mn, s = divmod(r, 60)
    return f"{int(h):02d}:{int(mn):02d}:{s:06.3f}".replace(".", sep)


base = os.path.splitext(output)[0]
with open(base + ".srt", "w") as f:
    for i, (m, a, b) in enumerate(wins):
        f.write(f"{i + 1}\n{ts(a, ',')} --> {ts(b, ',')}\n{m['text']}\n\n")
with open(base + ".vtt", "w") as f:
    f.write("WEBVTT\n\n")
    for m, a, b in wins:
        f.write(f"{ts(a, '.')} --> {ts(b, '.')}\n{m['text']}\n\n")

inputs = ["-i", src]
g, parts = [], []
for k, (a, b, fct) in enumerate(segs):
    g.append(f"[0:v]trim=start={a:.3f}:end={b:.3f},setpts=(PTS-STARTPTS)/{fct},fps=30[v{k}]")
    parts.append(f"[v{k}]")
g.append("".join(parts) + f"concat=n={len(segs)}:v=1:a=0[base]")
last, idx = "base", 1
for k, (m, a, b) in enumerate(wins):
    png = subs.get(m["id"])
    if not png:
        continue
    inputs += ["-loop", "1", "-i", png]
    g.append(f"[{last}][{idx}:v]overlay=x=(W-w)/2:y=H-h-26:enable='between(t,{a:.3f},{b:.3f})':shortest=1[s{k}]")
    last = f"s{k}"
    idx += 1
labels = []
for k, (m, a, b) in enumerate(wins):
    inputs += ["-i", files[m["id"]]]
    ms = int(a * 1000)
    g.append(f"[{idx}:a]adelay={ms}|{ms},aresample=48000[n{k}]")
    labels.append(f"[n{k}]")
    idx += 1
g.append("".join(labels) + f"amix=inputs={len(labels)}:normalize=0:dropout_transition=0,apad,atrim=0:{duration:.2f},pan=stereo|c0=c0|c1=c0[aud]")
inputs += ["-i", base + ".srt"]
enc = ["-c:v", "h264_videotoolbox", "-b:v", "8M"] if fast else ["-c:v", "libx264", "-preset", "medium", "-crf", "20"]
cmd = ["ffmpeg", "-y", "-v", "error"] + inputs + ["-filter_complex", ";".join(g), "-map", f"[{last}]", "-map", "[aud]", "-map", f"{idx}:s",
       *enc, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-c:s", "mov_text", "-metadata:s:s:0", "language=ita",
       "-disposition:s:0", "0", "-t", f"{duration:.2f}", "-movflags", "+faststart", output]
t0 = time.time()
subprocess.run(cmd, check=True)
print(f"wrote {output}: {duration:.0f} s, {len(marks)} lines, {len(speedups)} sped-up waits, {time.time() - t0:.0f} s")
