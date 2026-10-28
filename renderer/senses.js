// Sight and hearing. Camera frames go to a face detector running locally
// in WebAssembly; the microphone becomes a single loudness number.
// Nothing is recorded, stored, or sent anywhere.
import { FilesetResolver, FaceDetector } from '/node_modules/@mediapipe/tasks-vision/vision_bundle.mjs';

const WASM = '/node_modules/@mediapipe/tasks-vision/wasm';
const MODEL = '/assets/models/blaze_face_short_range.tflite';

async function makeDetector() {
  const fileset = await FilesetResolver.forVisionTasks(WASM);
  const options = (delegate) => ({
    baseOptions: { modelAssetPath: MODEL, delegate },
    runningMode: 'VIDEO',
    minDetectionConfidence: 0.5,
  });
  try {
    return await FaceDetector.createFromOptions(fileset, options('GPU'));
  } catch (err) {
    console.warn('GPU face detector failed, using CPU:', err.message);
    return await FaceDetector.createFromOptions(fileset, options('CPU'));
  }
}

export class Senses {
  constructor(video) {
    this.video = video;
    this.detector = null;
    this.videoStream = null;
    this.audioStream = null;
    this.audioCtx = null;
    this.analyser = null;
    this.samples = null;
    this.canSee = false;
    this.canHear = false;
    this.faces = [];
    this.level = 0;
    this.lastVideoTime = -1;
  }

  async open() {
    await Promise.all([this.openEyes(), this.openEars()]);
    console.log(`senses open: sight=${this.canSee} hearing=${this.canHear}`);
  }

  async openEyes() {
    try {
      this.videoStream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 640 }, height: { ideal: 480 }, facingMode: 'user' },
        audio: false,
      });
      this.video.srcObject = this.videoStream;
      await this.video.play();
      this.detector ??= await makeDetector();
      this.canSee = true;
    } catch (err) {
      console.warn('no sight:', err.name, err.message);
      this.canSee = false;
    }
  }

  async openEars() {
    try {
      this.audioStream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
        video: false,
      });
      this.audioCtx = new AudioContext();
      const src = this.audioCtx.createMediaStreamSource(this.audioStream);
      this.analyser = this.audioCtx.createAnalyser();
      this.analyser.fftSize = 1024;
      this.samples = new Float32Array(this.analyser.fftSize);
      src.connect(this.analyser);
      this.canHear = true;
    } catch (err) {
      console.warn('no hearing:', err.name, err.message);
      this.canHear = false;
    }
  }

  close() {
    for (const s of [this.videoStream, this.audioStream]) s?.getTracks().forEach((t) => t.stop());
    this.videoStream = this.audioStream = null;
    this.video.srcObject = null;
    this.audioCtx?.close();
    this.audioCtx = this.analyser = null;
    this.canSee = this.canHear = false;
    this.faces = [];
    this.level = 0;
  }

  // Faces as { x, y, size, score }: center in 0..1 of the raw camera frame
  // (not mirrored), size as a fraction of frame width.
  look() {
    const v = this.video;
    if (!this.canSee || !this.detector || v.readyState < 2) return this.faces;
    if (v.currentTime === this.lastVideoTime) return this.faces;
    this.lastVideoTime = v.currentTime;
    const result = this.detector.detectForVideo(v, performance.now());
    const w = v.videoWidth;
    const h = v.videoHeight;
    this.faces = result.detections
      .filter((d) => d.boundingBox)
      .map((d) => {
        const b = d.boundingBox;
        return {
          x: (b.originX + b.width / 2) / w,
          y: (b.originY + b.height / 2) / h,
          size: b.width / w,
          score: d.categories?.[0]?.score ?? 1,
        };
      })
      .sort((a, b) => b.size - a.size);
    return this.faces;
  }

  // Loudness 0..1 on a decibel scale: about -55 dB is silence, -12 dB is a shout.
  listen() {
    if (!this.analyser) return (this.level = 0);
    this.analyser.getFloatTimeDomainData(this.samples);
    let sum = 0;
    for (const s of this.samples) sum += s * s;
    const rms = Math.sqrt(sum / this.samples.length);
    const db = 20 * Math.log10(rms + 1e-9);
    this.level = Math.min(1, Math.max(0, (db + 55) / 43));
    return this.level;
  }
}
