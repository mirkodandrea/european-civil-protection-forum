#!/usr/bin/env python3
"""Compose the demo video in one ffmpeg pass.

Inputs
  <out>/video_start_ms.txt   wall-clock ms when the browser recording started
  <out>/*.webm               Playwright recording of play.html (480x1040), encoded while recording
  <out>/marks.json           narration marks from session.js: id, t (ms from video start), duration_s
  <out>/narration.json       narration lines with WAV files (tts.js)
  <out>/subs/*.png           burned-in subtitles and title (render_subs.js)
  <rec>/sim.mkv              the simulator window, encoded while recording (viz::record)
  <rec>/sim_start_ms.txt     wall-clock ms of the first simulator frame

Output: a 1920x1080 MP4 with the phone (left), the simulator (right), a title,
burned-in subtitles under the simulator, the narration as stereo AAC and a soft
subtitle track, plus .srt and .vtt next to it.

  python3 compose.py <out_dir> <rec_dir> <output.mp4> [--fast]

--fast uses the hardware H.264 encoder (a few times faster, bigger file).
"""
import glob
import json
import os
import subprocess
import sys
import time

out_dir, rec_dir, output = sys.argv[1:4]
fast = "--fast" in sys.argv
crf = sys.argv[sys.argv.index("--crf") + 1] if "--crf" in sys.argv else "30"

video_start = int(open(os.path.join(out_dir, "video_start_ms.txt")).read())
sim_start = int(open(os.path.join(rec_dir, "sim_start_ms.txt")).read())
webm = glob.glob(os.path.join(out_dir, "*.webm"))[0]
sim = os.path.join(rec_dir, "sim.mkv")
marks = json.load(open(os.path.join(out_dir, "marks.json")))
files = {l["id"]: l["file"] for l in json.load(open(os.path.join(out_dir, "narration.json")))["lines"]}
subs = sorted(glob.glob(os.path.join(out_dir, "subs", "[0-9][0-9]_*.png")))
sub_by_id = {os.path.basename(p)[3:-4]: p for p in subs}
title = os.path.join(out_dir, "subs", "title.png")

duration = float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", webm]).decode())
duration -= 0.5

# Subtitle windows: from the mark to the end of the line (or the next mark).
windows = []
for i, m in enumerate(marks):
    start = m["t"] / 1000
    end = start + m["duration_s"] + 0.3
    if i + 1 < len(marks):
        end = min(end, marks[i + 1]["t"] / 1000)
    windows.append((m, start, min(end, duration)))


def ts(x, sep):
    h, rem = divmod(x, 3600)
    mnt, s = divmod(rem, 60)
    return f"{int(h):02d}:{int(mnt):02d}:{s:06.3f}".replace(".", sep)


base = os.path.splitext(output)[0]
with open(base + ".srt", "w") as f:
    for i, (m, a, b) in enumerate(windows):
        f.write(f"{i + 1}\n{ts(a, ',')} --> {ts(b, ',')}\n{m['text']}\n\n")
with open(base + ".vtt", "w") as f:
    f.write("WEBVTT\n\n")
    for m, a, b in windows:
        f.write(f"{ts(a, '.')} --> {ts(b, '.')}\n{m['text']}\n\n")

inputs = ["-i", sim, "-i", webm, "-loop", "1", "-i", title]
graph = []
offset = (sim_start - video_start) / 1000
sim_chain = "[0:v]setpts=PTS-STARTPTS,fps=25,scale=1380:776:flags=lanczos"
sim_chain += f",tpad=start_duration={offset:.3f}:start_mode=clone" if offset > 0 else f",trim=start={-offset:.3f},setpts=PTS-STARTPTS"
graph.append(sim_chain + "[sim]")
graph.append("[1:v]fps=25,setpts=PTS-STARTPTS[phone]")
graph.append(f"color=c=0x141416:s=1920x1080:r=25:d={duration:.2f}[bg]")
graph.append("[bg][phone]overlay=x=20:y=20:eof_action=repeat[a0]")
graph.append("[a0][sim]overlay=x=520:y=152:eof_action=repeat[a1]")
graph.append("[a1][2:v]overlay=x=520:y=78:shortest=1[a2]")
last = "a2"
idx = 3
for k, (m, a, b) in enumerate(windows):
    png = sub_by_id.get(m["id"])
    if not png:
        continue
    inputs += ["-loop", "1", "-i", png]
    graph.append(f"[{last}][{idx}:v]overlay=x=520:y=936:enable='between(t,{a:.3f},{b:.3f})':shortest=1[s{k}]")
    last = f"s{k}"
    idx += 1
audio_labels = []
for k, m in enumerate(marks):
    inputs += ["-i", files[m["id"]]]
    ms = max(0, int(m["t"]))
    graph.append(f"[{idx}:a]adelay={ms}|{ms},aresample=48000[n{k}]")
    audio_labels.append(f"[n{k}]")
    idx += 1
graph.append("".join(audio_labels) + f"amix=inputs={len(audio_labels)}:normalize=0:dropout_transition=0,apad,atrim=0:{duration:.2f},pan=stereo|c0=c0|c1=c0[aud]")
inputs += ["-i", base + ".srt"]

enc = ["-c:v", "h264_videotoolbox", "-b:v", "3500k"] if fast else ["-c:v", "libx264", "-preset", "veryfast", "-crf", crf]
cmd = ["ffmpeg", "-y", "-v", "error"] + inputs + [
    "-filter_complex", ";".join(graph),
    "-map", f"[{last}]", "-map", "[aud]", "-map", f"{idx}:s",
    *enc, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-c:s", "mov_text", "-metadata:s:s:0", "language=eng",
    "-disposition:s:0", "0", "-t", f"{duration:.2f}", "-movflags", "+faststart", output,
]
t0 = time.time()
subprocess.run(cmd, check=True)
print(f"wrote {output}: {duration:.0f} s, {len(marks)} narration lines, encoded in {time.time() - t0:.0f} s ({'hardware' if fast else 'x264'})")
