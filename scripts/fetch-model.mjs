// Downloads the MediaPipe face detector model into assets/models.
// Runs once at setup; the app never touches the network after that.
import { mkdir, writeFile, access } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const MODEL_URL =
  'https://storage.googleapis.com/mediapipe-models/face_detector/blaze_face_short_range/float16/1/blaze_face_short_range.tflite';
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'assets', 'models', 'blaze_face_short_range.tflite');

try {
  await access(out);
  console.log('model already present:', out);
} catch {
  const res = await fetch(MODEL_URL);
  if (!res.ok) throw new Error(`model download failed: ${res.status} ${res.statusText}`);
  const buf = Buffer.from(await res.arrayBuffer());
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, buf);
  console.log(`model saved (${buf.length} bytes):`, out);
}
