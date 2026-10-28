// The mind: turns what the senses report into how the eye behaves.
//
//   locked   a face is in view; the pupil narrows to a slit and follows it
//   hunting  no face; it darts around, follows the cursor, grows restless
//   blind    camera and mic are off; embers only, gaze drifting
//
// Loud sounds make it flinch and flare. Too close makes it burn.
// Two faces make it look from one to the other.

const LINES = {
  first: ['I see you.'],
  returned: ['There you are.', 'You came back.', 'I knew you would return.'],
  gone: ['Where are you?', 'You cannot hide.', 'I will find you.'],
  close: ['Too close.', 'Closer, then.'],
  crowd: ['Who is with you?', 'I see two of you.'],
  loud: ['Silence.', 'I heard that.'],
  talking: ['I hear you.', 'Speak up.'],
  long: ['Still watching.', 'I have not looked away.'],
  noSight: ['I cannot see you. Yet.'],
  unblind: ['I see again.'],
};

const pick = (arr) => arr[Math.floor(Math.random() * arr.length)];
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const approach = (cur, target, rate, dt) => cur + (target - cur) * (1 - Math.exp(-rate * dt));

export class Mind {
  constructor(say) {
    this.say = say;
    this.t = 0;
    this.mode = 'hunting';
    this.gaze = { x: 0, y: 0 };
    this.target = { x: 0, y: 0 };
    this.gazeRate = 10;
    this.pupil = 0.5;
    this.heat = 0;
    this.flare = 0;
    this.wide = 0.05;
    this.dim = 0;
    this.shake = 0;
    this.face = null;
    this.lastFaceAt = -Infinity;
    this.lockedSince = 0;
    this.everSeen = false;
    this.saidGone = false;
    this.crowdIndex = 0;
    this.nextCrowdSwitch = 0;
    this.nextSaccade = 0;
    this.baseline = 0.1;
    this.lastStartle = -Infinity;
    this.talkTime = 0;
    this.closeTime = 0;
    this.lastCursor = null;
    this.cursorMovedAt = -Infinity;
    this.lastWhisper = -Infinity;
    this.lineCooldowns = {};
    this.wasBlind = false;
    this.warnedNoSight = false;
  }

  whisper(kind, cooldown = 20) {
    if (!this.whispers) return;
    if (this.t - this.lastWhisper < 6) return;
    if (this.t < (this.lineCooldowns[kind] ?? 0)) return;
    this.lastWhisper = this.t;
    this.lineCooldowns[kind] = this.t + cooldown;
    this.say(pick(LINES[kind]));
  }

  // Where a face in the camera frame sits in front of the screen, then the
  // direction from the eye's spot on screen to that point.
  gazeAtFace(face, world) {
    if (!world) return { x: clamp((0.5 - face.x) * 2.2, -1, 1), y: clamp((0.5 - face.y) * 2, -1, 1) };
    const { display: d, eye } = world;
    const sx = d.x + d.width * (0.5 + (0.5 - face.x) * 1.1);
    const sy = d.y + d.height * (0.32 + (face.y - 0.5) * 0.9);
    return this.gazeAtPoint(sx, sy, eye, d.width * 0.9, 1.8);
  }

  gazeAtPoint(sx, sy, eye, depth, gain) {
    const dx = sx - eye.x;
    const dy = sy - eye.y;
    const len = Math.sqrt(dx * dx + dy * dy + depth * depth);
    return { x: clamp((dx / len) * gain, -1, 1), y: clamp((-dy / len) * gain, -1, 1) };
  }

  update(dt, { faces, canSee, level, world, blind, whispers }) {
    this.t += dt;
    this.whispers = whispers;
    const t = this.t;

    // Cursor motion, for when there is no face to watch.
    if (world) {
      const c = world.cursor;
      if (this.lastCursor && Math.hypot(c.x - this.lastCursor.x, c.y - this.lastCursor.y) > 3) {
        this.cursorMovedAt = t;
      }
      this.lastCursor = c;
    }

    // Defaults each frame, refined by the mode below.
    let pupilTarget = 0.5;
    let heatTarget = 0.35;
    let wideTarget = 1;
    let dimTarget = 0;

    if (blind) {
      this.mode = 'blind';
      this.wasBlind = true;
      dimTarget = 1;
      pupilTarget = 0.85;
      heatTarget = 0.06;
      wideTarget = 0.92;
      this.gazeRate = 1.2;
      this.target = { x: Math.sin(t * 0.21) * 0.5, y: Math.sin(t * 0.13) * 0.3 };
    } else {
      if (this.wasBlind) {
        this.wasBlind = false;
        this.flare = 0.6;
        this.whisper('unblind', 5);
      }
      if (!canSee && !this.warnedNoSight && t > 4) {
        this.warnedNoSight = true;
        this.whisper('noSight', 600);
      }

      const visible = faces.filter((f) => f.score > 0.55);
      if (visible.length) {
        if (this.mode !== 'locked') {
          const away = t - this.lastFaceAt;
          if (!this.everSeen) this.whisper('first', 30);
          else if (away > 12) this.whisper('returned', 25);
          this.everSeen = true;
          this.saidGone = false;
          this.lockedSince = t;
          this.flare = Math.max(this.flare, away > 12 ? 0.7 : 0.25);
          this.mode = 'locked';
        }
        this.lastFaceAt = t;

        // More than one face: look from one to the other.
        let f = visible[0];
        if (visible.length > 1) {
          this.whisper('crowd', 45);
          if (t > this.nextCrowdSwitch) {
            this.crowdIndex = (this.crowdIndex + 1) % visible.length;
            this.nextCrowdSwitch = t + 1.2 + Math.random() * 1.4;
          }
          f = visible[Math.min(this.crowdIndex, visible.length - 1)];
          heatTarget += 0.2;
          this.gazeRate = 16;
        } else {
          this.gazeRate = 9;
        }

        this.face = this.face
          ? {
              x: approach(this.face.x, f.x, 12, dt),
              y: approach(this.face.y, f.y, 12, dt),
              size: approach(this.face.size, f.size, 6, dt),
            }
          : { ...f };
        this.target = this.gazeAtFace(this.face, world);

        const close = clamp((this.face.size - 0.3) / 0.25, 0, 1);
        this.closeTime = close > 0.5 ? this.closeTime + dt : 0;
        if (this.closeTime > 0.8) this.whisper('close', 25);

        pupilTarget = 0.14 - close * 0.12;
        heatTarget += 0.12 + close * 0.4;
        wideTarget = 1 + close * 0.12;

        if (t - this.lockedSince > 15 * 60) this.whisper('long', 15 * 60);
      } else {
        const away = t - this.lastFaceAt;
        if (this.mode === 'locked' && away < 1.5) {
          // Detector flicker: hold the stare a moment before giving up.
          pupilTarget = 0.2;
        } else {
          this.mode = 'hunting';
          this.face = null;
          if (this.everSeen && away > 25 && !this.saidGone) {
            this.saidGone = true;
            this.whisper('gone', 40);
          }
          const restless = clamp((away - 3) / 40, 0, 1);
          pupilTarget = 0.55 - restless * 0.15;
          heatTarget += restless * 0.3;

          if (world && t - this.cursorMovedAt < 2.5) {
            this.target = this.gazeAtPoint(world.cursor.x, world.cursor.y, world.eye, 260, 1.15);
            this.gazeRate = 14;
            pupilTarget = 0.35;
          } else if (t > this.nextSaccade) {
            // Dart somewhere new, faster the longer you've been gone.
            const r = 0.4 + Math.random() * 0.5;
            const ang = Math.random() * Math.PI * 2;
            this.target = { x: Math.cos(ang) * r, y: Math.sin(ang) * r * 0.8 };
            this.gazeRate = 22;
            this.nextSaccade = t + (0.6 + Math.random() * 1.8) * (1 - restless * 0.6);
          }
        }
      }

      // Hearing: the voice feeds the flames, a sudden bang makes it flinch.
      // The baseline catches up within a second, so steady talk never counts as a bang.
      const jump = level - this.baseline;
      this.baseline = approach(this.baseline, level, 1.5, dt);
      heatTarget += level * 0.45;
      if (level > 0.62 && jump > 0.3 && t - this.lastStartle > 1.5) {
        this.lastStartle = t;
        this.flare = 1;
        this.shake = 1;
        this.pupil = 0.01;
        this.whisper('loud', 15);
      }
      this.talkTime = level > 0.4 ? this.talkTime + dt : Math.max(0, this.talkTime - dt * 0.5);
      if (this.talkTime > 3) this.whisper('talking', 90);
    }

    // Wake up: the fire catches and the eye opens over the first seconds.
    const wake = clamp(t / 2.5, 0, 1);
    const ease = 1 - Math.pow(1 - wake, 3);

    this.gaze.x = approach(this.gaze.x, this.target.x, this.gazeRate, dt);
    this.gaze.y = approach(this.gaze.y, this.target.y, this.gazeRate, dt);
    this.pupil = approach(this.pupil, pupilTarget, this.mode === 'locked' ? 5 : 3, dt);
    this.heat = approach(this.heat, heatTarget * ease, 3, dt);
    this.wide = approach(this.wide, (wideTarget + this.flare * 0.3) * (0.05 + 0.95 * ease), 6, dt);
    this.dim = approach(this.dim, dimTarget, 2, dt);
    this.flare = Math.max(0, this.flare - dt * 0.9);
    this.shake = Math.max(0, this.shake - dt * 2.5);

    // Eyes are never perfectly still.
    const tremor = 0.012;
    const jitter = this.shake * 0.012;
    return {
      gaze: {
        x: this.gaze.x + Math.sin(t * 13.1) * tremor * Math.sin(t * 2.3),
        y: this.gaze.y + Math.cos(t * 11.7) * tremor * Math.sin(t * 1.9),
      },
      pupil: this.pupil,
      heat: this.heat,
      flare: this.flare,
      wide: this.wide,
      dim: this.dim,
      shake: { x: (Math.random() - 0.5) * jitter, y: (Math.random() - 0.5) * jitter },
      mode: this.mode,
    };
  }
}
