// Wires the senses, the mind, and the eye together and runs the frame loop.
import { createEye } from './eye.js';
import { Senses } from './senses.js';
import { Mind } from './mind.js';
import { createVoice } from './voice.js';

const bridge = window.unblinking;
const IS_MAC = bridge?.platform === 'darwin';
document.body.classList.add(bridge?.platform ?? 'web');

const canvas = document.getElementById('eye');
const whisperEl = document.getElementById('whisper');
const eye = createEye(canvas);
const senses = new Senses(document.getElementById('cam'));
const voice = createVoice();

// The latest of everything the main process tells us. Same keys as the
// prefs main.js sends; these defaults hold until the first message arrives.
let world = null;
let prefs = { whispers: true, blind: false, voice: false };
let trustedLevel = 0;
let whisperTimer = null;

// Every whisper passes through here: kind is the LINES key in mind.js.
function say(text, kind) {
  console.log('whisper:', text);
  whisperEl.textContent = text;
  whisperEl.classList.add('show');
  clearTimeout(whisperTimer);
  whisperTimer = setTimeout(() => whisperEl.classList.remove('show'), 3200);
  if (prefs.voice) voice.say(text);
}

const mind = new Mind(say);

// Open the camera and mic only once we know it isn't meant to be blind.
// Blinding and unblinding can arrive quickly; each change waits for the last.
let started = false;
let sensesBusy = Promise.resolve();
function setSight(open) {
  sensesBusy = sensesBusy
    .then(() => (open ? senses.open() : senses.close()))
    .catch((err) => console.error('senses failed:', err.message));
  return sensesBusy;
}

function start() {
  if (started) return;
  started = true;
  if (!prefs.blind) setSight(true);
}

bridge?.onWorld((w) => (world = w));
bridge?.onPrefs((p) => {
  const wasBlind = prefs.blind;
  prefs = { ...prefs, ...p };
  voice.setEnabled(!!prefs.voice);
  if (!started) return start();
  if (prefs.blind && !wasBlind) setSight(false);
  if (!prefs.blind && wasBlind) setSight(true);
});
// Without a word from the main process (or outside Electron), open anyway.
setTimeout(start, 1500);

// The eye sits a little above center so whispers fit below it.
// On a Mac it hangs from the menu bar, so it sits higher still.
const center = IS_MAC ? { x: 0, y: 0.1 } : { x: 0, y: 0.06 };

let last = performance.now();
let lastLook = 0;
let lookFailed = false;
function frame(now) {
  // Ask for the next frame first, so one bad frame never stops the eye.
  requestAnimationFrame(frame);
  const dt = Math.min(0.1, (now - last) / 1000);
  last = now;

  // Face detection is the costly part; 15 times a second is plenty.
  if (now - lastLook > 66) {
    lastLook = now;
    try {
      senses.look();
      lookFailed = false;
    } catch (err) {
      // Log once per run of failures, not 15 times a second.
      if (!lookFailed) console.error('look failed:', err.message);
      lookFailed = true;
    }
  }
  // The mic hears the speakers, so while the eye's own sound may reach it
  // (speaking, or the drone loud or swelling) the mind keeps the last level it trusted.
  // Holding rather than zeroing keeps the bang detector's baseline steady.
  const heard = senses.listen();
  if (!voice.masking) trustedLevel = heard;
  const level = trustedLevel;

  // s: the mind's state this frame (gaze, pupil, heat, flare, wide, dim, shake, mode).
  const s = mind.update(dt, {
    faces: senses.faces,
    canSee: senses.canSee,
    level,
    world,
    blind: prefs.blind,
    whispers: prefs.whispers,
  });

  eye.render({
    flow: (now / 1000 / 4) % 1,
    gaze: s.gaze,
    pupil: s.pupil,
    heat: s.heat,
    flare: s.flare,
    wide: s.wide,
    dim: s.dim,
    center: { x: center.x + s.shake.x, y: center.y + s.shake.y },
  });
  voice.update(s, dt);
}
requestAnimationFrame(frame);

// Debug heartbeat in the terminal: what it sees and hears.
setInterval(() => {
  const f = senses.faces[0];
  console.log(
    `mode=${mind.mode} faces=${senses.faces.length}` +
      (f ? ` face=(${f.x.toFixed(2)},${f.y.toFixed(2)}) size=${f.size.toFixed(2)}` : '') +
      ` level=${senses.level.toFixed(2)}`
  );
}, 3000);

// Let go of the camera, the mic, and the speakers when the page goes away.
window.addEventListener('pagehide', () => {
  senses.close();
  voice.dispose();
});
