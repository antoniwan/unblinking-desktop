// Sight and hearing. Camera frames go to a face detector running locally
// in WebAssembly; the microphone becomes a single loudness number.
// Nothing is recorded, stored, or sent anywhere.
//
// Every sense can fail on its own (no camera, mic denied, model missing).
// A missing sense only leaves canSee / canHear false; the eye keeps running.
import { FilesetResolver, FaceDetector } from '/node_modules/@mediapipe/tasks-vision/vision_bundle.mjs';

const WASM = '/node_modules/@mediapipe/tasks-vision/wasm';
const MODEL = '/assets/models/blaze_face_short_range.tflite';

async function makeDetector() {
  // Fail early and plainly if setup never fetched the model.
  const res = await fetch(MODEL).catch(() => null);
  if (!res?.ok) throw new Error(`face model missing at ${MODEL}; run \`npm run setup\``);

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

const stopStream = (stream) => stream?.getTracks().forEach((t) => t.stop());

export class Senses {
  constructor(video) {
    this.video = video;
    this.detector = null;
    this.detectorFailed = false;
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
    // Bumped by close(). An open() still awaiting the camera when the eye is
    // blinded sees the change and lets go of whatever it was handed.
    this.session = 0;
  }

  async open() {
    if (!navigator.mediaDevices?.getUserMedia) {
      console.warn('no senses: this page has no access to media devices');
      return;
    }
    await Promise.all([this.openEyes(), this.openEars()]);
    console.log(`senses open: sight=${this.canSee} hearing=${this.canHear}`);
  }

  async openEyes() {
    const session = this.session;
    let stream = null;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 640 }, height: { ideal: 480 }, facingMode: 'user' },
        audio: false,
      });
      if (session !== this.session) return stopStream(stream);
      this.videoStream = stream;
      this.video.srcObject = stream;
      await this.video.play();
      if (!this.detector && !this.detectorFailed) {
        try {
          this.detector = await makeDetector();
        } catch (err) {
          // The model will not appear while the app runs; don't retry on every unblind.
          this.detectorFailed = true;
          console.error('face detector failed to load:', err.message);
        }
      }
      if (session !== this.session) return;
      if (!this.detector) {
        // A camera without a detector sees nothing, so don't keep its light on.
        this.closeEyes();
        return;
      }
      // A camera unplugged mid-run ends its track; stop pretending to see.
      stream.getVideoTracks()[0]?.addEventListener('ended', () => {
        if (this.videoStream !== stream) return;
        console.warn('sight lost: camera track ended');
        this.closeEyes();
      });
      this.canSee = true;
    } catch (err) {
      console.warn('no sight:', err.name, err.message);
      if (session === this.session) this.closeEyes();
      else stopStream(stream);
    }
  }

  async openEars() {
    const session = this.session;
    let stream = null;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
        video: false,
      });
      if (session !== this.session) return stopStream(stream);
      this.audioStream = stream;
      this.audioCtx = new AudioContext();
      const src = this.audioCtx.createMediaStreamSource(stream);
      this.analyser = this.audioCtx.createAnalyser();
      this.analyser.fftSize = 1024;
      this.samples = new Float32Array(this.analyser.fftSize);
      src.connect(this.analyser);
      stream.getAudioTracks()[0]?.addEventListener('ended', () => {
        if (this.audioStream !== stream) return;
        console.warn('hearing lost: mic track ended');
        this.closeEars();
      });
      this.canHear = true;
    } catch (err) {
      console.warn('no hearing:', err.name, err.message);
      if (session === this.session) this.closeEars();
      else stopStream(stream);
    }
  }

  close() {
    this.session++;
    this.closeEyes();
    this.closeEars();
  }

  closeEyes() {
    stopStream(this.videoStream);
    this.videoStream = null;
    this.video.pause();
    this.video.srcObject = null;
    this.lastVideoTime = -1;
    this.canSee = false;
    this.faces = [];
  }

  closeEars() {
    stopStream(this.audioStream);
    this.audioStream = null;
    this.audioCtx?.close().catch(() => {});
    this.audioCtx = this.analyser = this.samples = null;
    this.canHear = false;
    this.level = 0;
  }

  // Faces as { x, y, size, score }: center in 0..1 of the raw camera frame
  // (not mirrored), size as a fraction of frame width.
  look() {
    const v = this.video;
    if (!this.canSee || !this.detector || v.readyState < 2 || !v.videoWidth) return this.faces;
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
