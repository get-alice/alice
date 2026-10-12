import { gsap } from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import { SplitText } from 'gsap/SplitText';
import { burst } from './nature';

gsap.registerPlugin(ScrollTrigger, SplitText);
ScrollTrigger.config({ ignoreMobileResize: true });

const one = <T extends Element = HTMLElement>(selector: string, root: ParentNode = document) => root.querySelector<T>(selector)!;
const all = <T extends Element = HTMLElement>(selector: string, root: ParentNode = document) => [...root.querySelectorAll<T>(selector)];
const shown = (el: Element) => getComputedStyle(el).display !== 'none';

// The comparison slider works with or without motion.
const glass = one('[data-glass]');
const splitInput = one<HTMLInputElement>('[data-split-input]');
const setSplit = (value: number) => {
  glass.style.setProperty('--split', `${value}%`);
  splitInput.value = String(Math.round(value));
};
splitInput.addEventListener('input', () => setSplit(+splitInput.value));

// SplitText needs final font metrics; don't wait forever on a slow font.
const fontsReady = Promise.race([document.fonts.ready, new Promise(resolve => setTimeout(resolve, 1500))]);

fontsReady.then(() => {
  gsap.matchMedia().add(
    { motion: '(prefers-reduced-motion: no-preference)', wide: '(min-width: 60rem)', fine: '(hover: hover) and (pointer: fine)' },
    context => {
      const { motion, wide, fine } = context.conditions as Record<string, boolean>;
      document.documentElement.classList.toggle('motion', motion);
      if (!motion) return;
      const cleanups = [hero(wide, fine), headings(), reveals(), product(), mirror(wide), cheshire(wide), bento(), privacy(wide), download()];
      return () => cleanups.forEach(cleanup => cleanup?.());
    },
  );
});

/** Inbox noise hangs in the sky; scroll sends a breeze that blows it away as petals, and the flower opens in the sun. */
function hero(wide: boolean, fine: boolean) {
  const section = one('[data-hero]');
  const frags = all('.frag', section).filter(shown);
  const chips = frags.map(frag => one('.chip', frag));
  const vortex = one('[data-vortex]', section);
  const sun = one('.sun', vortex);
  const moon = one('.moon', vortex);
  const bloom = one('.bloom', vortex);
  const petals = all('.petal', vortex);
  const heart = one('.heart', vortex);
  const clouds = all('.cloud', section);
  const title = one('h1', section);
  const rise = all('[data-rise]', section);

  frags.forEach((frag, i) => gsap.set(frag, {
    left: `${wide ? frag.dataset.x : i === 1 ? 18 : 40}%`, top: wide ? `${frag.dataset.y}%` : `${7 + i * 2}rem`, xPercent: -50, yPercent: -50, scale: 0.55 + +frag.dataset.z! * 0.5,
  }));
  gsap.set(sun, { scale: 0.55, autoAlpha: 0.55 });
  gsap.set(bloom, { rotation: -120, scale: 0.7 });
  gsap.set(petals, { attr: { rx: 0, ry: 0 } });
  gsap.set(heart, { attr: { r: 0 } });

  const split = SplitText.create(title, { type: 'lines,words', mask: 'lines' });
  gsap.set([title, ...rise], { visibility: 'visible' });
  gsap.timeline({ defaults: { ease: 'expo.out' } })
    .from(clouds, { autoAlpha: 0, x: -60, duration: 2.4, stagger: 0.15, clearProps: 'opacity,visibility' }, 0) // hand opacity back to CSS so the theme switch can change it
    .from(moon, { autoAlpha: 0, y: 24, rotation: -25, duration: 2.4 }, 0.3)
    .from(chips, { autoAlpha: 0, scale: 0.4, duration: 1.6, stagger: { each: 0.035, from: 'random' } }, 0.1)
    .from(split.words, { yPercent: 115, duration: 1.3, stagger: 0.07, onComplete: () => split.revert() }, 0.2)
    .from(rise, { autoAlpha: 0, y: 24, duration: 1.1, stagger: 0.1 }, 0.7);

  // Each message bobs on its own slow breath; clouds idle along.
  chips.forEach(chip => gsap.to(chip, {
    x: gsap.utils.random(-10, 10), y: gsap.utils.random(-14, 14), duration: gsap.utils.random(3, 5.5),
    ease: 'sine.inOut', yoyo: true, repeat: -1, delay: 1.4,
  }));
  clouds.forEach(cloud => gsap.to(cloud, { xPercent: gsap.utils.random(18, 40), duration: gsap.utils.random(14, 24), ease: 'sine.inOut', yoyo: true, repeat: -1 }));

  // The breeze: left to right, each message lifts away and leaves a few petals behind.
  const breeze = gsap.timeline({
    scrollTrigger: { trigger: section, start: 'top top', end: wide ? '+=110%' : 'bottom top', pin: wide, scrub: 0.8 },
  });
  frags.forEach(frag => {
    const x = +frag.dataset.x!;
    const z = +frag.dataset.z!;
    breeze.to(frag, {
      x: () => gsap.utils.random(0.35, 0.8) * innerWidth, y: () => gsap.utils.random(-0.55, -0.2) * innerHeight,
      rotation: gsap.utils.random(-25, 45), scale: 0.2, autoAlpha: 0, ease: 'power1.in', duration: 0.5 + (1 - z) * 0.2,
      onStart: () => {
        const box = frag.getBoundingClientRect();
        if (box.width) burst(box.left + box.width / 2, box.top + box.height / 2, z > 0.6 ? 4 : 2);
      },
    }, (x / 100) * 0.45);
  });
  breeze.to(sun, { scale: 1.15, autoAlpha: 1, duration: 0.9, ease: 'power2.out' }, 0.45)
    .to(bloom, { rotation: 0, scale: 1, duration: 0.8, ease: 'power3.out' }, 0.75)
    .to(petals, { attr: { rx: (i: number) => (i % 2 ? 14 : 7), ry: (i: number) => (i % 2 ? 7 : 14) }, duration: 0.55, ease: 'back.out(1.7)', stagger: 0.07 }, 0.78)
    .to(heart, { attr: { r: 5 }, duration: 0.35, ease: 'back.out(2)' }, 1.05);

  if (!fine) return;
  // Depth: nearer layers and clouds follow the pointer further.
  const follow = (el: HTMLElement, reach: number) => ({
    x: gsap.quickTo(el, 'x', { duration: 1.4, ease: 'power3' }), y: gsap.quickTo(el, 'y', { duration: 1.4, ease: 'power3' }), reach,
  });
  const movers = [
    ...all('.layer', section).map((layer, i) => follow(layer, 8 + i * 12)),
    ...clouds.map(cloud => follow(cloud, 30 * +cloud.dataset.depth!)),
  ];
  const move = (event: PointerEvent) => {
    const nx = event.clientX / innerWidth - 0.5;
    const ny = event.clientY / innerHeight - 0.5;
    movers.forEach(m => { m.x(-nx * m.reach * 2); m.y(-ny * m.reach * 1.5); });
  };
  section.addEventListener('pointermove', move);
  return () => section.removeEventListener('pointermove', move);
}

/** Section titles rise line by line as they arrive. */
function headings() {
  all('[data-split]').filter(el => !el.closest('[data-hero]')).forEach(el => {
    const split = SplitText.create(el, { type: 'lines,words', mask: 'lines' });
    gsap.set(el, { visibility: 'visible' });
    gsap.from(split.words, {
      yPercent: 115, duration: 1.2, ease: 'expo.out', stagger: 0.06,
      scrollTrigger: { trigger: el, start: 'top 86%', once: true },
      onComplete: () => split.revert(),
    });
  });
}

function reveals() {
  const items = all('[data-reveal]');
  gsap.set(items, { autoAlpha: 0, y: 28 });
  ScrollTrigger.batch(items, {
    start: 'top 90%', once: true,
    onEnter: batch => gsap.to(batch, { autoAlpha: 1, y: 0, duration: 1.1, ease: 'expo.out', stagger: 0.08 }),
  });
}

/** The app tilts up from the floor as it comes into view. */
function product() {
  gsap.fromTo('[data-tilt]', { rotationX: 30, scale: 0.86, y: 60 }, {
    rotationX: 0, scale: 1, y: 0, ease: 'none',
    scrollTrigger: { trigger: '[data-tilt-stage]', start: 'top 95%', end: 'top 20%', scrub: 0.6 },
  });
}

/** Scroll sweeps the glass from the app to the terminal and back to the middle; then it's yours to drag. */
function mirror(wide: boolean) {
  if (!wide) return;
  const state = { v: 100 };
  const apply = () => setSplit(state.v);
  apply();
  gsap.timeline({ scrollTrigger: { trigger: '[data-mirror]', start: 'top top', end: '+=150%', pin: true, scrub: 0.6 } })
    .to(state, { v: 0, duration: 1, ease: 'power2.inOut', onUpdate: apply })
    .to(state, { v: 50, duration: 0.6, ease: 'power2.inOut', onUpdate: apply });

}

/** Ask Alice materialises word by word, holds, then fades, leaving only its grin. */
function cheshire(wide: boolean) {
  const section = one('[data-cheshire]');
  const ask = one('[data-ask]', section);
  const words = all('[data-words]', section).flatMap(el => SplitText.create(el, { type: 'words' }).words);
  gsap.set(words, { opacity: 0.08 });
  const trigger = wide
    ? { trigger: one('.grid', section), start: 'top top', end: '+=170%', pin: true, scrub: 0.5 }
    : { trigger: ask, start: 'top 85%', end: 'bottom 15%', scrub: 0.5 };
  gsap.timeline({ scrollTrigger: trigger })
    .to(words, { opacity: 1, stagger: 0.04, duration: 0.3, ease: 'none' })
    .to({}, { duration: 1.2 })
    .to(words, { opacity: 0, stagger: { each: 0.012, from: 'random' }, duration: 0.3 })
    .to(ask, { backgroundColor: 'rgba(0,0,0,0)', borderColor: 'rgba(0,0,0,0)', duration: 0.6 }, '<0.4');
}

/** Small loops that only run while their tile is on screen. */
function bento() {
  const caps = all('[data-keys] kbd');
  const keys = gsap.timeline({ repeat: -1, repeatDelay: 0.9, paused: true });
  caps.forEach((cap, i) => keys
    .call(() => cap.classList.add('down'), [], i * 0.5)
    .call(() => cap.classList.remove('down'), [], i * 0.5 + 0.2));

  const query = one('[data-query]');
  const hit = one('[data-hit]');
  const text = query.textContent ?? '';
  const typed = { n: 0 };
  const type = () => { query.textContent = text.slice(0, Math.round(typed.n)); };
  const search = gsap.timeline({ repeat: -1, repeatDelay: 1, paused: true })
    .set(hit, { autoAlpha: 0, y: 8 })
    .fromTo(typed, { n: 0 }, { n: text.length, duration: text.length * 0.09, ease: 'none', onUpdate: type })
    .to(hit, { autoAlpha: 1, y: 0, duration: 0.6, ease: 'expo.out' }, '+=0.3')
    .to({}, { duration: 2.4 })
    .to(hit, { autoAlpha: 0, duration: 0.3 })
    .to(typed, { n: 0, duration: 0.5, ease: 'none', onUpdate: type });

  const whileVisible = (trigger: string, animation: gsap.core.Timeline) => ScrollTrigger.create({
    trigger, start: 'top bottom', end: 'bottom top', onToggle: self => (self.isActive ? animation.play() : animation.pause()),
  });
  whileVisible('[data-keys]', keys);
  whileVisible('[data-search]', search);
}

/** The wire draws from your provider to you; the server in between is struck out. */
function privacy(wide: boolean) {
  const section = one('[data-private]');
  const wire = one('[data-wire]', section);
  const packet = one('[data-packet]', section);
  const axis = wide ? 'scaleX' : 'scaleY';
  gsap.timeline({ scrollTrigger: { trigger: wire, start: 'top 80%', end: 'top 40%', scrub: 0.6 } })
    .fromTo(wire, { [axis]: 0 }, { [axis]: 1, ease: 'none', duration: 1 })
    .fromTo(one('[data-strike]', section), { scaleX: 0 }, { scaleX: 1, ease: 'power2.inOut', duration: 0.5 }, 0.5)
    .to(one('[data-ghost]', section), { autoAlpha: 0.4, y: wide ? -6 : 0, duration: 0.4 }, 0.9);

  const length = () => (wide ? wire.clientWidth : wire.clientHeight);
  const travel = gsap.timeline({ repeat: -1, repeatDelay: 0.5, paused: true })
    .fromTo(packet, { [wide ? 'x' : 'y']: 0 }, { [wide ? 'x' : 'y']: length, duration: 1.8, ease: 'power1.inOut' })
    .fromTo(packet, { opacity: 0 }, { opacity: 1, duration: 0.25 }, 0)
    .to(packet, { opacity: 0, duration: 0.25 }, 1.55);
  ScrollTrigger.create({ trigger: wire, start: 'top 60%', end: 'bottom top', onToggle: self => (self.isActive ? travel.play() : travel.pause()) });
}

/** The mark blooms at the finale and keeps turning with the scroll. */
function download() {
  const mark = one('[data-download] .mark');
  gsap.timeline({ scrollTrigger: { trigger: mark, start: 'top 88%', once: true }, defaults: { ease: 'expo.out' } })
    .from(all('.petal', mark), { attr: { rx: 0, ry: 0 }, duration: 1.4, stagger: 0.08 })
    .from(one('.heart', mark), { attr: { r: 0 }, duration: 1 }, 0.3);
  gsap.fromTo(mark, { rotation: -60 }, { rotation: 60, ease: 'none', scrollTrigger: { trigger: '[data-download]', start: 'top bottom', end: 'bottom top', scrub: 1 } });
}
