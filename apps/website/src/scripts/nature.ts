/**
 * The page's weather: petals and seeds in the air (fireflies at night), a meadow that bends away
 * from the pointer, and a dandelion you can blow on. Plain canvas; one shared animation loop that
 * only runs while something is on screen. With reduced motion every scene draws one still frame.
 */

type Scene = { visible: boolean; draw: (t: number) => void; resize: () => void };

const still = matchMedia('(prefers-reduced-motion: reduce)');
const night = { get matches() { return document.documentElement.dataset.theme === 'dark'; } };
const scenes: Scene[] = [];
const rand = (min: number, max: number) => min + Math.random() * (max - min);
const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
// Theme colours, read once per theme rather than per frame.
const theme = { seed: '', stem: '' };
const readTheme = () => { theme.seed = css('--seed'); theme.stem = css('--grass-near'); };

// Pointer in viewport coordinates, with a velocity that eases back to calm.
const pointer = { x: -1e4, y: -1e4, vx: 0, vy: 0, seen: false };
addEventListener('pointermove', event => {
  if (pointer.seen) {
    pointer.vx = Math.max(-40, Math.min(40, event.clientX - pointer.x));
    pointer.vy = Math.max(-40, Math.min(40, event.clientY - pointer.y));
  }
  pointer.x = event.clientX;
  pointer.y = event.clientY;
  pointer.seen = true;
}, { passive: true });
document.documentElement.addEventListener('pointerleave', () => { pointer.x = pointer.y = -1e4; pointer.seen = false; });

function fit(canvas: HTMLCanvasElement) {
  const ratio = Math.min(devicePixelRatio || 1, 2);
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  return { ctx, width, height };
}

let frame = 0;
function loop(t: number) {
  frame = 0;
  if (!theme.seed) readTheme(); // stylesheets can land after this module in development
  let any = false;
  for (const scene of scenes) if (scene.visible) { scene.draw(t); any = true; }
  pointer.vx *= 0.88;
  pointer.vy *= 0.88;
  if (any && !still.matches) frame = requestAnimationFrame(loop);
}
const wake = () => { if (!frame) frame = requestAnimationFrame(loop); };

function register(canvas: HTMLCanvasElement, scene: Omit<Scene, 'visible'>) {
  const entry: Scene = { ...scene, visible: false };
  scenes.push(entry);
  entry.resize();
  new IntersectionObserver(([e]) => { entry.visible = e.isIntersecting; if (entry.visible) wake(); }).observe(canvas);
  new ResizeObserver(() => { entry.resize(); if (still.matches) entry.draw(performance.now()); }).observe(canvas);
  return entry;
}

/* ---------------- Air: petals and seeds by day, fireflies by night ---------------- */

type Mote = { x: number; y: number; vx: number; vy: number; rot: number; spin: number; size: number; depth: number; phase: number; kind: 0 | 1; tint: string; life?: number };
let burstQueue: { x: number; y: number; n: number }[] = [];

/** Scatter a few petals from a point on screen (viewport coordinates). */
export function burst(x: number, y: number, n = 3) {
  burstQueue.push({ x, y, n });
  wake();
}

function air(canvas: HTMLCanvasElement) {
  let ctx: CanvasRenderingContext2D;
  let width = 0;
  let height = 0;
  let motes: Mote[] = [];
  let lastScroll = scrollY;
  let palette: string[] = [];
  const make = (x = rand(0, width), y = rand(0, height)): Mote => {
    return {
      x, y, vx: rand(0.1, 0.5), vy: rand(-0.1, 0.25), rot: rand(0, 6.28), spin: rand(-0.02, 0.02),
      size: rand(3, 7), depth: rand(0.4, 1), phase: rand(0, 6.28), kind: !night.matches && Math.random() < 0.3 ? 1 : 0,
      tint: palette[Math.floor(Math.random() * palette.length)],
    };
  };
  const seed = () => {
    const count = Math.round(Math.max(14, Math.min(42, (width * height) / 38000)));
    motes = Array.from({ length: count }, () => make());
  };
  register(canvas, {
    resize() {
      ({ ctx, width, height } = fit(canvas));
      palette = night.matches ? ['#ffe9a6'] : [css('--lavender'), '#ffffff', '#f4c9dc', '#c9b8ec'];
      seed();
    },
    draw(t) {
      ctx.clearRect(0, 0, width, height);
      const scrolled = scrollY - lastScroll;
      lastScroll = scrollY;
      for (const b of burstQueue) for (let i = 0; i < b.n; i++) {
        const m = make(b.x + rand(-20, 20), b.y + rand(-10, 10));
        m.kind = 0; m.vx = rand(1, 3.5); m.vy = rand(-2.5, -0.5); m.life = 1; m.depth = 1;
        motes.push(m);
      }
      burstQueue = [];
      const breeze = 0.35 + Math.sin(t * 0.00025) * 0.25;
      motes = motes.filter(m => {
        if (night.matches) {
          // Fireflies wander and drift toward a still pointer.
          m.vx += Math.sin(t * 0.0007 + m.phase) * 0.01;
          m.vy += Math.cos(t * 0.0009 + m.phase * 1.3) * 0.01;
          const dx = pointer.x - m.x, dy = pointer.y - m.y, d = Math.hypot(dx, dy);
          if (d < 220 && d > 20) { m.vx += (dx / d) * 0.012; m.vy += (dy / d) * 0.012; }
          m.vx *= 0.985; m.vy *= 0.985;
        } else {
          m.vx += (breeze * m.depth - m.vx) * 0.01;
          m.vy += (0.18 * m.depth - m.vy) * 0.008;
          const dx = m.x - pointer.x, dy = m.y - pointer.y, d = Math.hypot(dx, dy);
          if (d < 150) {
            const k = 1 - d / 150;
            m.vx += pointer.vx * 0.05 * k + (dx / (d || 1)) * 0.5 * k;
            m.vy += pointer.vy * 0.05 * k + (dy / (d || 1)) * 0.5 * k;
            m.spin += pointer.vx * 0.0006 * k;
          }
        }
        m.spin *= 0.995;
        m.x += m.vx + Math.sin(t * 0.0011 + m.phase) * 0.25 * m.depth;
        m.y += m.vy - scrolled * 0.25 * m.depth;
        m.rot += m.spin + 0.004;
        if (m.life !== undefined) { m.life -= 0.004; if (m.life <= 0) return false; }
        else {
          if (m.x > width + 30) { m.x = -30; m.y = rand(0, height); }
          if (m.x < -40) m.x = width + 30;
          if (m.y > height + 30) { m.y = -30; m.x = rand(0, width); }
          if (m.y < -40) m.y = height + 30;
        }
        paint(m, t);
        return true;
      });
    },
  });
  function paint(m: Mote, t: number) {
    const fade = m.life === undefined ? 1 : Math.min(1, m.life * 2);
    ctx.save();
    ctx.translate(m.x, m.y);
    if (night.matches) {
      const glow = (0.45 + 0.55 * Math.sin(t * 0.002 + m.phase * 3)) * fade;
      const r = m.size * 2.2 * m.depth;
      const g = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
      g.addColorStop(0, `rgba(255, 236, 170, ${0.9 * glow})`);
      g.addColorStop(0.25, `rgba(255, 226, 140, ${0.35 * glow})`);
      g.addColorStop(1, 'rgba(255, 226, 140, 0)');
      ctx.fillStyle = g;
      ctx.beginPath(); ctx.arc(0, 0, r, 0, 6.29); ctx.fill();
    } else if (m.kind === 1) {
      drawSeed(ctx, m.rot * 0.2, 0.75 * m.depth, `rgba(255, 255, 255, ${0.9 * fade})`);
    } else {
      ctx.rotate(m.rot);
      ctx.scale(1, 0.35 + 0.65 * Math.abs(Math.cos(m.rot * 1.7 + m.phase)));
      ctx.globalAlpha = 0.85 * fade;
      ctx.fillStyle = m.tint;
      ctx.beginPath(); ctx.ellipse(0, 0, m.size * m.depth, m.size * 0.55 * m.depth, 0, 0, 6.29); ctx.fill();
    }
    ctx.restore();
  }
}

/** A floating dandelion seed: a thin stalk below a fan of feathery hairs. Drawn around (0,0). */
function drawSeed(ctx: CanvasRenderingContext2D, tilt: number, scale: number, color: string) {
  ctx.save();
  ctx.rotate(tilt);
  ctx.scale(scale, scale);
  ctx.strokeStyle = color;
  ctx.lineWidth = 0.8 / scale;
  ctx.beginPath();
  ctx.moveTo(0, 0); ctx.lineTo(0, 16);
  for (let k = -4; k <= 4; k++) {
    const a = -Math.PI / 2 + k * 0.3;
    ctx.moveTo(0, 0); ctx.lineTo(Math.cos(a) * 10, Math.sin(a) * 10);
  }
  ctx.stroke();
  ctx.fillStyle = color;
  ctx.beginPath(); ctx.ellipse(0, 17.5, 1, 2.2, 0, 0, 6.29); ctx.fill();
  ctx.restore();
}

/* ---------------- Meadow: grass and small flowers that lean away from the pointer ---------------- */

type Blade = { x: number; layer: number; tall: number; width: number; lean: number; phase: number; bend: number; vel: number; flower: string | null };

function meadow(canvas: HTMLCanvasElement) {
  let ctx: CanvasRenderingContext2D;
  let width = 0;
  let height = 0;
  let blades: Blade[] = [];
  let colors: string[] = [];
  let ground = '';
  register(canvas, {
    resize() {
      ({ ctx, width, height } = fit(canvas));
      colors = [css('--grass-far'), css('--grass-mid'), css('--grass-near')];
      ground = 'ground' in canvas.dataset ? css('--ground') : '';
      const blooms = night.matches ? ['#8d7cb8', '#c9c3dc', '#6e5f99'] : [css('--lavender'), '#ffffff', '#f4c9dc', '#f6d66b'];
      blades = Array.from({ length: Math.round(width / 2.6) }, () => {
        const layer = Math.random();
        return {
          x: rand(-10, width + 10), layer, tall: height * (0.3 + layer * 0.62) * rand(0.65, 1), width: 2 + layer * 4,
          lean: rand(-0.18, 0.18), phase: rand(0, 6.28), bend: 0, vel: 0,
          flower: Math.random() < 0.035 ? blooms[Math.floor(Math.random() * blooms.length)] : null,
        };
      }).sort((a, b) => a.layer - b.layer);
    },
    draw(t) {
      const box = canvas.getBoundingClientRect();
      const px = pointer.x - box.left;
      const py = pointer.y - box.top;
      ctx.clearRect(0, 0, width, height);
      if (ground) {
        // Thicken the turf where the meadow meets solid ground below.
        const soil = ctx.createLinearGradient(0, height * 0.45, 0, height);
        soil.addColorStop(0, `${ground}00`); // --ground is a 6-digit hex
        soil.addColorStop(1, ground);
        ctx.fillStyle = soil;
        ctx.fillRect(0, height * 0.45, width, height * 0.55);
      }
      for (const b of blades) {
        const wind = Math.sin(t * 0.0011 + b.x * 0.006) * 0.1 + Math.sin(t * 0.0004 + b.x * 0.0017) * 0.12;
        let target = 0;
        const dx = b.x - px;
        if (Math.abs(dx) < 130 && py > height - b.tall - 80 && py < height + 40) {
          const k = 1 - Math.abs(dx) / 130;
          target = Math.sign(dx) * k * 0.75 + pointer.vx * 0.012 * k;
        }
        b.vel = (b.vel + (target - b.bend) * 0.06) * 0.86;
        b.bend += b.vel;
        const sway = b.lean + wind * (0.5 + b.layer * 0.5) + b.bend;
        const tipX = b.x + Math.sin(sway) * b.tall;
        const tipY = height - Math.cos(sway) * b.tall;
        const midX = b.x + Math.sin(sway * 0.4) * b.tall * 0.5;
        const midY = height - b.tall * 0.55;
        ctx.fillStyle = colors[Math.min(2, Math.floor(b.layer * 3))];
        ctx.beginPath();
        ctx.moveTo(b.x - b.width / 2, height);
        ctx.quadraticCurveTo(midX - b.width * 0.3, midY, tipX, tipY);
        ctx.quadraticCurveTo(midX + b.width * 0.3, midY, b.x + b.width / 2, height);
        ctx.fill();
        if (b.flower) flower(ctx, tipX, tipY, 2.6 + b.layer * 3.4, b.flower, t * 0.0005 + b.phase);
      }
    },
  });
}

/** The Alice mark in miniature: four petals and a warm centre. */
function flower(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, color: string, turn: number) {
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(Math.sin(turn) * 0.3);
  ctx.fillStyle = color;
  for (let i = 0; i < 4; i++) {
    ctx.rotate(Math.PI / 2);
    ctx.beginPath(); ctx.ellipse(0, -r * 0.62, r * 0.42, r * 0.7, 0, 0, 6.29); ctx.fill();
  }
  ctx.fillStyle = '#f6d66b';
  ctx.beginPath(); ctx.arc(0, 0, r * 0.32, 0, 6.29); ctx.fill();
  ctx.restore();
}

/* ---------------- Dandelion: blow on it and the seeds float off, then regrow ---------------- */

type Tuft = { dx: number; dy: number; depth: number; state: 'on' | 'flying' | 'gone'; x: number; y: number; vx: number; vy: number; rot: number; grow: number; away: number };

function dandelion(canvas: HTMLCanvasElement) {
  const section = canvas.closest('section')!;
  const spot = section.querySelector<HTMLElement>('[data-dandelion-spot]')!;
  let ctx: CanvasRenderingContext2D;
  let width = 0;
  let height = 0;
  let cx = 0, cy = 0, radius = 0, groundY = 0;
  let last = 0;
  let nextShed = 0;
  const count = 120;
  // Points on a sphere, seen from the front: their screen offset and how much they face us.
  const tufts = Array.from({ length: count }, (_, i): Tuft => {
    const z = 1 - (2 * (i + 0.5)) / count;
    const r = Math.sqrt(1 - z * z);
    const phi = i * 2.39996;
    return { dx: Math.cos(phi) * r, dy: Math.sin(phi) * r, depth: z, state: 'on', x: 0, y: 0, vx: 0, vy: 0, rot: 0, grow: 1, away: 0 };
  }).sort((a, b) => a.depth - b.depth);

  const release = (t: Tuft, vx: number, vy: number) => {
    if (t.state !== 'on' || t.grow < 1) return;
    t.state = 'flying';
    t.x = cx + t.dx * radius;
    t.y = cy + t.dy * radius;
    t.vx = vx + t.dx * 1.2;
    t.vy = vy + t.dy * 0.8 - 0.4;
    t.rot = Math.atan2(t.dy, t.dx) + Math.PI / 2;
  };
  /** A puff of wind from the left, as if someone blew on it. */
  const puff = (share: number) => {
    for (const t of tufts) if (Math.random() < share) release(t, rand(1.5, 3.5), rand(-1.6, -0.2));
    wake();
  };
  section.addEventListener('click', event => {
    if ((event.target as Element).closest('a, button')) return;
    puff(0.35);
  });

  let blown = false;
  new IntersectionObserver(([e]) => {
    if (e.intersectionRatio > 0.5 && !blown && !still.matches) { blown = true; setTimeout(() => puff(0.18), 500); }
  }, { threshold: 0.5 }).observe(spot);

  register(canvas, {
    resize() {
      ({ ctx, width, height } = fit(canvas));
      const s = spot.getBoundingClientRect();
      const c = canvas.getBoundingClientRect();
      radius = Math.min(s.width, s.height) * 0.26;
      cx = s.left - c.left + s.width / 2;
      cy = s.top - c.top + radius * 1.25;
      groundY = s.bottom - c.top;
    },
    draw(t) {
      const dt = Math.min(50, t - (last || t));
      last = t;
      const box = canvas.getBoundingClientRect();
      const px = pointer.x - box.left;
      const py = pointer.y - box.top;
      const speed = Math.hypot(pointer.vx, pointer.vy);
      if (t > nextShed) { nextShed = t + rand(1800, 3200); const on = tufts.filter(f => f.state === 'on'); if (on.length > 20) release(on[Math.floor(Math.random() * on.length)], rand(0.6, 1.4), rand(-0.6, 0)); }

      ctx.clearRect(0, 0, width, height);
      const seedColor = theme.seed;
      const sway = Math.sin(t * 0.0009) * 0.04;
      const headX = cx + Math.sin(sway) * (groundY - cy);

      // Stem
      ctx.strokeStyle = theme.stem;
      ctx.lineWidth = 3;
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.moveTo(cx, groundY);
      ctx.quadraticCurveTo(cx - radius * 0.15, (groundY + cy) / 2, headX, cy);
      ctx.stroke();

      for (const f of tufts) {
        if (f.state === 'on') {
          if (f.grow < 1) f.grow = Math.min(1, f.grow + dt / 1400);
          const tipX = headX + f.dx * radius * f.grow;
          const tipY = cy + f.dy * radius * f.grow;
          if (speed > 3 && Math.hypot(tipX - px, tipY - py) < 46) release(f, pointer.vx * 0.12, pointer.vy * 0.12);
          const alpha = (0.35 + 0.45 * (f.depth + 1) / 2) * f.grow;
          ctx.strokeStyle = `rgb(${seedColor} / ${alpha})`;
          ctx.lineWidth = 0.7;
          ctx.beginPath();
          ctx.moveTo(headX + f.dx * radius * 0.12, cy + f.dy * radius * 0.12);
          ctx.lineTo(tipX, tipY);
          // The hairs open toward the viewer: a full star when facing us, a fan when side-on.
          const spread = 0.3 + (1 - Math.hypot(f.dx, f.dy)) * 0.5;
          const out = Math.atan2(f.dy, f.dx);
          const hair = radius * 0.2 * f.grow;
          for (let k = -4; k <= 4; k++) {
            const a = out + k * spread;
            ctx.moveTo(tipX, tipY); ctx.lineTo(tipX + Math.cos(a) * hair, tipY + Math.sin(a) * hair);
          }
          ctx.stroke();
        } else if (f.state === 'flying') {
          f.vx += (0.7 - f.vx) * 0.006;
          f.vy += (-0.35 - f.vy) * 0.006;
          const dx = f.x - px, dy = f.y - py, d = Math.hypot(dx, dy);
          if (d < 120) { const k = 1 - d / 120; f.vx += pointer.vx * 0.03 * k; f.vy += pointer.vy * 0.03 * k; }
          f.x += f.vx + Math.sin(t * 0.002 + f.depth * 9) * 0.35;
          f.y += f.vy;
          f.rot += (Math.sin(t * 0.0015 + f.depth * 7) * 0.25 - f.rot) * 0.03;
          ctx.save();
          ctx.translate(f.x, f.y);
          drawSeed(ctx, f.rot, radius / 50, `rgb(${seedColor} / 0.75)`);
          ctx.restore();
          if (f.x > width + 40 || f.y < -40 || f.x < -40) { f.state = 'gone'; f.away = t + rand(3500, 8000); }
        } else if (t > f.away) {
          f.state = 'on';
          f.grow = 0;
        }
      }
      // The head's centre.
      ctx.fillStyle = `rgb(${seedColor} / 0.55)`;
      ctx.beginPath(); ctx.arc(headX, cy, radius * 0.1, 0, 6.29); ctx.fill();
    },
  });
}

const airCanvas = document.querySelector<HTMLCanvasElement>('[data-air]');
if (airCanvas && !still.matches) air(airCanvas);
document.querySelectorAll<HTMLCanvasElement>('[data-meadow]').forEach(meadow);
document.querySelectorAll<HTMLCanvasElement>('[data-dandelion]').forEach(dandelion);
// Reduced motion: each scene draws one still frame.
if (still.matches) requestAnimationFrame(t => { readTheme(); scenes.forEach(scene => scene.draw(t)); });
/** Repaint every scene in the current theme, right now (the theme switch captures the page straight after). */
export function refreshTheme() {
  readTheme();
  scenes.forEach(scene => { scene.resize(); scene.draw(performance.now()); });
  wake();
}
new MutationObserver(refreshTheme).observe(document.documentElement, { attributeFilter: ['data-theme'] });
