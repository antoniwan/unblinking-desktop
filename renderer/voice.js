// The voice: a low drone under the fire, and whispers spoken aloud.
// Everything is made here with Web Audio and the system's own speech voices.
// No audio files, nothing fetched.
//
// The drone is quiet at rest and swells (louder, brighter) with heat and flare.
// The mic hears the speakers (senses.js turns echo cancellation off), so the
// drone stays low and ducks while it speaks; see `speaking`.

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const approach = (cur, target, rate, dt) => cur + (target - cur) * (1 - Math.exp(-rate * dt));

// Drone loudness at rest and at full burn. Kept well below what the mic calls talking.
const REST_GAIN = 0.025;
const PEAK_GAIN = 0.11;
// Filter cutoff in Hz, closed at rest, open at full burn.
const REST_CUTOFF = 140;
const PEAK_CUTOFF = 900;
// How often parameters are pushed to the audio thread.
const PUSH_EVERY = 1 / 15;
// The room keeps ringing a moment after the voice stops.
const SPEECH_TAIL = 0.6;

// Deep, plain voices first; bright ones last. Names vary by OS.
const DEEP = ['david', 'mark', 'guy', 'george', 'daniel', 'fred', 'ralph', 'bruce', 'alex', 'thomas', 'male'];
const BRIGHT = ['zira', 'hazel', 'susan', 'samantha', 'victoria', 'karen', 'moira', 'tessa', 'female', 'whisper', 'bells', 'bubbles'];

function pickVoice(voices) {
  const score = (v) => {
    const name = v.name.toLowerCase();
    let s = 0;
    if (v.localService === false) s -= 100; // a remote voice would leave the machine
    if (/^en(-|_|$)/i.test(v.lang)) s += 10;
    if (DEEP.some((n) => name.includes(n))) s += 5;
    if (BRIGHT.some((n) => name.includes(n))) s -= 5;
    return s;
  };
  const best = [...voices].sort((a, b) => score(b) - score(a))[0];
  return best && best.localService !== false ? best : null;
}

function brownNoise(ctx, seconds) {
  const len = Math.floor(ctx.sampleRate * seconds);
  const buf = ctx.createBuffer(1, len, ctx.sampleRate);
  const data = buf.getChannelData(0);
  let last = 0;
  for (let i = 0; i < len; i++) {
    last = (last + (Math.random() * 2 - 1) * 0.02) / 1.02;
    data[i] = last * 3.5;
  }
  return buf;
}

function buildDrone(ctx) {
  const master = ctx.createGain();
  master.gain.value = 0;
  master.connect(ctx.destination);

  const filter = ctx.createBiquadFilter();
  filter.type = 'lowpass';
  filter.frequency.value = REST_CUTOFF;
  filter.Q.value = 4;
  filter.connect(master);

  // A slow breath on the filter, so the drone never sits perfectly still.
  const lfo = ctx.createOscillator();
  lfo.frequency.value = 0.07;
  const lfoDepth = ctx.createGain();
  lfoDepth.gain.value = 40;
  lfo.connect(lfoDepth).connect(filter.frequency);

  // Detuned low oscillators: a root, a slightly sour twin, a sub, a faint fifth.
  const voices = [
    { type: 'sawtooth', freq: 55, detune: 0, gain: 0.35 },
    { type: 'sawtooth', freq: 55, detune: 11, gain: 0.3 },
    { type: 'triangle', freq: 27.5, detune: -4, gain: 0.5 },
    { type: 'sine', freq: 82.4, detune: 6, gain: 0.08 },
  ];
  const oscs = voices.map((v) => {
    const o = ctx.createOscillator();
    o.type = v.type;
    o.frequency.value = v.freq;
    o.detune.value = v.detune;
    const g = ctx.createGain();
    g.gain.value = v.gain;
    o.connect(g).connect(filter);
    return o;
  });

  // Filtered noise: the hiss of the fire.
  const noise = ctx.createBufferSource();
  noise.buffer = brownNoise(ctx, 4);
  noise.loop = true;
  const band = ctx.createBiquadFilter();
  band.type = 'bandpass';
  band.frequency.value = 220;
  band.Q.value = 0.8;
  const hiss = ctx.createGain();
  hiss.gain.value = 0.15;
  noise.connect(band).connect(hiss).connect(master);

  const sources = [lfo, noise, ...oscs];
  sources.forEach((s) => s.start());
  return { master, filter, lfo, band, hiss, sources };
}

export function createVoice() {
  const synth = window.speechSynthesis ?? null;
  let ctx = null;
  let drone = null;
  let enabled = false;
  let suspendTimer = null;
  let unlock = null;
  let chosen = null;
  let speakingNow = false;
  let quietAt = -Infinity; // performance.now() seconds when the last line ended
  let swell = 0;
  let duck = 0;
  let sincePush = Infinity;

  const refreshVoice = () => {
    if (synth) chosen = pickVoice(synth.getVoices());
  };
  refreshVoice();
  synth?.addEventListener('voiceschanged', refreshVoice);

  // Autoplay policy: resume now, and if the page still says no, on the next click.
  function wake() {
    if (!ctx || ctx.state === 'running') return;
    ctx.resume().catch(() => {});
    if (unlock) return;
    unlock = () => {
      if (enabled) ctx?.resume().catch(() => {});
      window.removeEventListener('pointerdown', unlock);
      unlock = null;
    };
    window.addEventListener('pointerdown', unlock);
  }

  function setEnabled(on) {
    on = !!on;
    if (on === enabled) return;
    enabled = on;
    clearTimeout(suspendTimer);
    if (on) {
      if (!ctx) {
        ctx = new AudioContext({ latencyHint: 'playback' });
        drone = buildDrone(ctx);
      }
      wake();
      sincePush = Infinity;
    } else {
      synth?.cancel();
      speakingNow = false;
      if (!ctx) return;
      // Fade out before suspending so it doesn't click.
      const g = drone.master.gain;
      g.cancelScheduledValues(ctx.currentTime);
      g.setTargetAtTime(0, ctx.currentTime, 0.05);
      suspendTimer = setTimeout(() => ctx?.suspend().catch(() => {}), 300);
    }
  }

  // state is what Mind.update returns: { heat, flare, dim, mode, ... }.
  function update(state, dt) {
    if (!enabled || !drone || !state) return;
    const target = clamp(state.heat * 0.55 + state.flare * 0.8, 0, 1) * (1 - state.dim * 0.75);
    // Swell up fast, sink back slowly.
    swell = approach(swell, target, target > swell ? 6 : 1.2, dt);
    duck = approach(duck, speaking() ? 1 : 0, speaking() ? 8 : 1.5, dt);

    sincePush += dt;
    if (sincePush < PUSH_EVERY) return;
    sincePush = 0;

    const now = ctx.currentTime;
    const gain = (REST_GAIN + (PEAK_GAIN - REST_GAIN) * swell * swell) * (1 - duck * 0.7);
    drone.master.gain.setTargetAtTime(gain, now, 0.12);
    drone.filter.frequency.setTargetAtTime(REST_CUTOFF * Math.pow(PEAK_CUTOFF / REST_CUTOFF, swell), now, 0.15);
    drone.hiss.gain.setTargetAtTime(0.15 + swell * 0.35, now, 0.2);
    drone.band.frequency.setTargetAtTime(220 + swell * 500, now, 0.2);
    // Hunting makes it breathe faster; blind makes it sleep.
    const breath = state.mode === 'hunting' ? 0.16 : state.mode === 'blind' ? 0.04 : 0.07;
    drone.lfo.frequency.setTargetAtTime(breath, now, 1.5);
  }

  // Speak a line in a low, slow voice. Never queue: if it is still talking, skip.
  function say(text) {
    if (!enabled || !synth || !text) return false;
    if (synth.speaking || synth.pending) return false;
    if (!chosen) refreshVoice();
    const u = new SpeechSynthesisUtterance(text);
    if (chosen) {
      u.voice = chosen;
      u.lang = chosen.lang;
    }
    u.pitch = 0.3;
    u.rate = 0.72;
    u.volume = 0.6;
    const done = () => {
      speakingNow = false;
      quietAt = performance.now() / 1000;
    };
    u.onstart = () => (speakingNow = true);
    u.onend = done;
    u.onerror = done;
    speakingNow = true; // duck from the moment it is asked, not when the engine gets around to it
    synth.speak(u);
    return true;
  }

  // True while a line is being spoken, plus a short tail for the room to go quiet.
  // The mic hears the speakers, so callers should not trust loudness while this is true.
  function speaking() {
    return speakingNow || performance.now() / 1000 - quietAt < SPEECH_TAIL;
  }

  function dispose() {
    enabled = false;
    clearTimeout(suspendTimer);
    synth?.cancel();
    synth?.removeEventListener('voiceschanged', refreshVoice);
    if (unlock) window.removeEventListener('pointerdown', unlock);
    unlock = null;
    drone?.sources.forEach((s) => {
      try {
        s.stop();
      } catch {}
    });
    ctx?.close().catch(() => {});
    ctx = drone = null;
  }

  return {
    setEnabled,
    update,
    say,
    dispose,
    get speaking() {
      return speaking();
    },
    get enabled() {
      return enabled;
    },
  };
}
