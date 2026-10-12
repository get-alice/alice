import { refreshTheme } from './nature';

// Day/night switch. The inline script in <head> already chose the first theme; this flips and remembers it.
const root = document.documentElement;
const button = document.querySelector<HTMLButtonElement>('[data-theme-toggle]')!;
const meta = document.querySelector('meta[name="theme-color"]')!;
const label = () => button.setAttribute('aria-label', root.dataset.theme === 'dark' ? 'Switch to day' : 'Switch to night');
label();
new MutationObserver(label).observe(root, { attributeFilter: ['data-theme'] });

function set(theme: 'light' | 'dark') {
  root.dataset.theme = theme;
  meta.setAttribute('content', theme === 'dark' ? '#060914' : '#8fc8f0');
  refreshTheme();
}

button.addEventListener('click', () => {
  const next = root.dataset.theme === 'dark' ? 'light' : 'dark';
  try { localStorage.setItem('theme', next); } catch { /* private mode: the choice lasts for this page */ }
  if (!document.startViewTransition || matchMedia('(prefers-reduced-motion: reduce)').matches) return set(next);
  // The new sky opens as a circle from the switch.
  // Percentages keep the circle on the switch whatever the snapshot's pixel scale; 150% covers any corner.
  const box = button.getBoundingClientRect();
  const x = ((box.left + box.width / 2) / innerWidth) * 100;
  const y = ((box.top + box.height / 2) / innerHeight) * 100;
  const transition = document.startViewTransition(() => set(next));
  transition.finished.catch(() => {});
  transition.ready.then(() => root.animate(
    { clipPath: [`circle(0% at ${x}% ${y}%)`, `circle(150% at ${x}% ${y}%)`] },
    { duration: 800, easing: 'cubic-bezier(0.23, 1, 0.32, 1)', pseudoElement: '::view-transition-new(root)' },
  )).catch(() => { /* skipped (hidden tab, rapid clicks): the theme has still changed */ });
});
