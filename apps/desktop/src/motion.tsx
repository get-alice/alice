import { motion, type Transition } from 'motion/react';

/*
 * Motion vocabulary shared by every animated component. Values follow Emil Kowalski's guidance:
 * strong custom curves, UI motion under 300ms, enter with ease-out, never from scale(0),
 * and full `transform` strings (Motion's x/y/scale shorthands are not hardware-accelerated).
 * Reduced motion: <MotionConfig reducedMotion="user"> in main.tsx disables layout animation (the
 * gliding highlights), but it does not recognise full transform strings, so every travelling
 * transform goes through `travel`, which keeps the fade and drops the movement.
 */
type Bezier = [number, number, number, number];
export const easeOut: Bezier = [0.23, 1, 0.32, 1];
export const easeDrawer: Bezier = [0.32, 0.72, 0, 1];

/** The default for every Motion animation (set on MotionConfig). */
export const snappy: Transition = { duration: 0.22, ease: easeOut };
/** Exits are quicker than entrances: the system is responding, not the user deciding. */
export const leave: Transition = { duration: 0.16, ease: easeOut };

/** True when the user asked the system for less motion. Read at render so a system change applies to the next animation. */
export function prefersReducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** `from` normally; the resting `to` transform when the user prefers reduced motion, so only opacity animates. */
export function travel(from: string, to: string) {
  return prefersReducedMotion() ? to : from;
}

/** Content arriving: a short rise from 8px below. Spread at render (`{...rise}`) so the getter sees the current preference. */
export const rise = {
  get initial() { return { opacity: 0, transform: travel('translateY(8px)', 'translateY(0px)') }; },
  animate: { opacity: 1, transform: 'translateY(0px)' },
};

/**
 * One highlight shared by a group of options. Rendered inside the active option only; when the
 * active option changes, Motion's shared layout animation glides it from the old option to the new.
 */
export function Glide({ group, className = '' }: { group: string; className?: string }) {
  return <motion.span layoutId={group} className={`indicator ${className}`} aria-hidden="true" transition={snappy} />;
}
