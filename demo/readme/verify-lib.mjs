import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { DEMO_SPEC } from './storyboard.mjs';

export function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function probe(file, ffprobePath) {
  const raw = execFileSync(ffprobePath, [
    '-v', 'error',
    '-count_frames',
    '-select_streams', 'v:0',
    '-show_entries', 'stream=codec_name,width,height,pix_fmt,r_frame_rate,avg_frame_rate,nb_frames,nb_read_frames,duration:format=duration',
    '-of', 'json',
    file,
  ], { encoding: 'utf8' });
  const parsed = JSON.parse(raw);
  return { ...parsed.format, ...parsed.streams[0] };
}

function uint24(buffer, offset) {
  return buffer[offset] | (buffer[offset + 1] << 8) | (buffer[offset + 2] << 16);
}

function probeAnimatedWebp(file) {
  const buffer = readFileSync(file);
  if (buffer.toString('ascii', 0, 4) !== 'RIFF' || buffer.toString('ascii', 8, 12) !== 'WEBP') {
    throw new Error(`${path.basename(file)}: invalid animated WebP RIFF header`);
  }
  let width = 0;
  let height = 0;
  let frames = 0;
  let durationMs = 0;
  for (let offset = 12; offset + 8 <= buffer.length;) {
    const fourcc = buffer.toString('ascii', offset, offset + 4);
    const size = buffer.readUInt32LE(offset + 4);
    const payload = offset + 8;
    if (payload + size > buffer.length) throw new Error(`${path.basename(file)}: truncated ${fourcc} chunk`);
    if (fourcc === 'VP8X' && size >= 10) {
      width = uint24(buffer, payload + 4) + 1;
      height = uint24(buffer, payload + 7) + 1;
    }
    if (fourcc === 'ANMF' && size >= 16) {
      frames += 1;
      durationMs += uint24(buffer, payload + 12);
    }
    offset = payload + size + (size % 2);
  }
  if (!width || !height || !frames || !durationMs) throw new Error(`${path.basename(file)}: incomplete animated WebP metadata`);
  return {
    codec_name: 'webp',
    width,
    height,
    duration: durationMs / 1000,
    nb_frames: frames,
    r_frame_rate: `${DEMO_SPEC.output.fps}/1`,
  };
}

function rateValue(rate) {
  const [numerator, denominator] = String(rate ?? '').split('/').map(Number);
  return denominator > 0 ? numerator / denominator : Number.NaN;
}

export function verifyMediaSet(root, ffprobePath) {
  const required = ['demo.gif', 'demo.webp', 'demo.mp4'];
  const reports = {};
  for (const name of required) {
    const file = path.join(root, name);
    const stats = statSync(file);
    if (stats.size < 1024) throw new Error(`${name} is unexpectedly small (${stats.size} bytes)`);
    const media = name.endsWith('.webp') ? probeAnimatedWebp(file) : probe(file, ffprobePath);
    const width = Number(media.width);
    const height = Number(media.height);
    const frames = Number(media.nb_frames ?? media.nb_read_frames);
    const rate = rateValue(media.avg_frame_rate ?? media.r_frame_rate);
    const duration = Number.isFinite(Number(media.duration)) ? Number(media.duration) : frames / rate;
    if (width !== DEMO_SPEC.output.width) throw new Error(`${name}: expected width ${DEMO_SPEC.output.width}, got ${width}`);
    if (height !== 600) throw new Error(`${name}: expected 16:10 output height 600, got ${height}`);
    if (!Number.isFinite(duration) || duration < DEMO_SPEC.presentationMinimumSeconds || duration > DEMO_SPEC.presentationMaximumSeconds) {
      throw new Error(`${name}: expected ${DEMO_SPEC.presentationMinimumSeconds}-${DEMO_SPEC.presentationMaximumSeconds}s, got ${duration}`);
    }
    reports[name] = {
      bytes: stats.size,
      sha256: sha256(file),
      codec: media.codec_name,
      width,
      height,
      durationSeconds: Number(duration.toFixed(3)),
      pixelFormat: media.pix_fmt ?? null,
      frameRate: media.r_frame_rate ?? null,
      frames: Number.isFinite(frames) ? frames : null,
    };
  }
  if (reports['demo.mp4'].codec !== 'h264') throw new Error(`demo.mp4: expected h264, got ${reports['demo.mp4'].codec}`);
  if (reports['demo.mp4'].pixelFormat !== 'yuv420p') throw new Error(`demo.mp4: expected yuv420p, got ${reports['demo.mp4'].pixelFormat}`);
  return reports;
}
