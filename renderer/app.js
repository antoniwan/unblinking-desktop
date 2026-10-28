// Wires the senses, the mind, and the eye together and runs the frame loop.
import { createEye } from './eye.js';
import { Senses } from './senses.js';
import { Mind } from './mind.js';

const bridge = window.unblinking;
const IS_MAC = bridge?.platform === 'darwin';
document.body.classList.add(bridge?.platform ?? 'web');

const canvas = document.getElementById('eye');
const whisperEl = document.getElementById('whisper');
const eye = createEye(canvas);
const senses = new Senses(document.getElementById('cam'));

let world = null;
let prefs = { whispers: true, blind: false };
let whisperTimer = null;

function say(text) {
  console.log('whisper:', text);
  whisperEl.textContent = text;
  whisperEl.classList.add('show');
  clearTimeout(whisperTimer);
  whisperTimer = setTimeout(() => whisperEl.classList.remove('show'), 3200);
}

const mind = new Mind(say);

// Open the camera and mic only once we know it isn't meant to be blind.
let started = false;
function start() {
  if (started) return;
  started = true;
  if (!prefs.blind) senses.open();
}

bridge?.onWorld((w) => (world = w));
bridge?.onPrefs(async (p) => {
  const wasBlind = prefs.blind;
  prefs = p;
  if (!started) return start();
  if (p.blind && !wasBlind) senses.close();
  if (!p.blind && wasBlind) await senses.open();
});
setTimeout(start, 1500);

// The eye sits a little above center so whispers fit below it.
// On a Mac it hangs from the menu bar, so it sits higher still.
const center = IS_MAC ? { x: 0, y: 0.1 } : { x: 0, y: 0.06 };

let last = performance.now();
let lastLook = 0;
function frame(now) {
  const dt = Math.min(0.1, (now - last) / 1000);
  last = now;

  // Face detection is the costly part; 15 times a second is plenty.
  if (now - lastLook > 66) {
    lastLook = now;
    try {
      senses.look();
    } catch (err) {
      console.error('look failed:', err.message);
    }
  }
  const level = senses.listen();

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
  requestAnimationFrame(frame);
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
