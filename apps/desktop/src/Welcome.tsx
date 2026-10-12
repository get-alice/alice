import { useEffect, useRef, useState } from 'react';
import { motion } from 'motion/react';
import { ChevronRight } from 'lucide-react';
import { t } from './i18n';
import { easeOut, prefersReducedMotion, travel } from './motion';

/*
 * First-launch welcome, after the website hero: the inbox noise hangs in the sky, a breeze carries it
 * off left to right, the sun warms and Alice's flower opens. A once-per-install moment, so it is
 * allowed far more time than UI motion. Reduced motion keeps the fades, drops the travel and the noise.
 */

// x/y place each message in the window (%), z is depth (1 = nearest).
const noise = [
  { key: 'sale', x: 14, y: 16, z: 0.9 },
  { key: 'unread', x: 36, y: 9, z: 0.55 },
  { key: 'planning', x: 64, y: 14, z: 1 },
  { key: 'badge', x: 88, y: 24, z: 0.5 },
  { key: 'follow_up', x: 22, y: 38, z: 0.7 },
  { key: 'trial', x: 80, y: 42, z: 0.85 },
  { key: 'no_subject', x: 9, y: 62, z: 0.45 },
  { key: 'standup', x: 86, y: 66, z: 0.6 },
  { key: 'digest', x: 26, y: 84, z: 0.75 },
  { key: 'out_of_office', x: 70, y: 88, z: 0.4 },
] as const;

const breeze = 1.5; // seconds until the wind starts; it reaches the right edge 0.5s later
const bloom = 2.05; // the flower opens as the last messages leave
const petals = [{ cx: 32, cy: 23, rx: 7, ry: 14 }, { cx: 41, cy: 32, rx: 14, ry: 7 }, { cx: 32, cy: 41, rx: 7, ry: 14 }, { cx: 23, cy: 32, rx: 14, ry: 7 }];

/** One message: pops in, hangs, then lifts away up and to the right, scattering petals. Values are fixed so renders stay pure. */
function Message({ text, x, y, z, index }: { text: string; x: number; y: number; z: number; index: number }) {
  const arrive = 0.1 + ((index * 7) % 10) * 0.05;
  const leave = breeze + (x / 100) * 0.5;
  const fly = 0.5 + (1 - z) * 0.4;
  const total = leave + fly - arrive;
  const scale = 0.7 + z * 0.35;
  const rest = `translate(0vw, 0vh) rotate(0deg) scale(${scale})`;
  return <div className={`welcome-frag${z < 0.5 ? ' far' : ''}`} style={{ left: `${x}%`, top: `${y}%` }}>
    <motion.div className="welcome-chip"
      initial={{ opacity: 0, transform: `translate(0vw, 0vh) rotate(0deg) scale(${scale * 0.5})` }}
      animate={{
        opacity: [0, 1, 1, 0],
        transform: [`translate(0vw, 0vh) rotate(0deg) scale(${scale * 0.5})`, rest, rest,
          `translate(${24 + (100 - x) * 0.35}vw, ${-12 - z * 18}vh) rotate(${(index % 3) * 22 - 8}deg) scale(${scale * 0.3})`],
      }}
      transition={{ duration: total, delay: arrive, times: [0, 0.7 / total, (leave - arrive) / total, 1], ease: [easeOut, 'linear', [0.55, 0, 1, 0.45]] }}>
      {text}
    </motion.div>
    {Array.from({ length: z > 0.6 ? 4 : 2 }, (_, petal) => {
      // Fixed pseudo-random spread per petal: a long drift with the wind, a little lift or fall, a few turns.
      const seed = index * 4 + petal;
      const spread = (k: number) => ((seed * k) % 97) / 97;
      const dx = 120 + spread(53) * 220, dy = -70 + spread(31) * 150, turn = 180 + spread(17) * 360;
      return <motion.span key={petal} className="welcome-petal"
        initial={{ opacity: 0, transform: 'translate(0px, 0px) rotate(0deg)' }}
        animate={{ opacity: [0, 1, 1, 0], transform: ['translate(0px, 0px) rotate(0deg)', `translate(${dx * 0.3}px, ${dy * 0.5 - 18}px) rotate(${turn * 0.3}deg)`, `translate(${dx * 0.75}px, ${dy * 0.8}px) rotate(${turn * 0.75}deg)`, `translate(${dx}px, ${dy}px) rotate(${turn}deg)`] }}
        transition={{ duration: 1.8 + spread(11) * 0.8, delay: leave + petal * 0.05, times: [0, 0.15, 0.7, 1], ease: 'easeOut' }} />;
    })}
  </div>;
}

export default function Welcome({ onContinue }: { onContinue: () => void }) {
  const [calm] = useState(prefersReducedMotion);
  const button = useRef<HTMLButtonElement>(null);
  // Enter continues straight away, so nobody has to wait out the animation.
  useEffect(() => { button.current?.focus({ preventScroll: true }); }, []);
  const at = (seconds: number) => (calm ? seconds * 0.25 : seconds);
  const enter = (delay: number, duration = 1) => ({
    initial: { opacity: 0, transform: travel('translateY(18px)', 'translateY(0px)') },
    animate: { opacity: 1, transform: 'translateY(0px)' },
    transition: { duration, delay: at(delay), ease: easeOut },
  });
  const words = (text: string, from: number) => text.split(' ').map((word, i) => <span key={word} className="welcome-word">
    <motion.span initial={{ opacity: calm ? 0 : 1, transform: travel('translateY(110%)', 'translateY(0%)') }} animate={{ opacity: 1, transform: 'translateY(0%)' }}
      transition={{ duration: 1.1, delay: at(from + i * 0.07), ease: easeOut }}>{word}</motion.span>
  </span>);
  const lead = t('settings.welcome.title_lead').split(' ').length;
  return <div className="welcome">
    <header className="welcome-header" data-tauri-drag-region />
    <div className="welcome-clouds" aria-hidden="true"><span /><span /><span /></div>
    {!calm && <div className="welcome-noise" aria-hidden="true">
      {noise.map((message, index) => <Message key={message.key} index={index} text={t(`settings.welcome.noise.${message.key}`)} x={message.x} y={message.y} z={message.z} />)}
    </div>}
    <main className="welcome-stage">
      <div className="welcome-bloom" aria-hidden="true">
        <motion.div className="welcome-sun" initial={{ opacity: 0.4, transform: travel('scale(0.55)', 'scale(1)') }} animate={{ opacity: 1, transform: 'scale(1)' }}
          transition={{ duration: 1.6, delay: at(breeze + 0.2), ease: easeOut }} />
        <motion.svg viewBox="0 0 64 64" initial={{ opacity: 0, transform: travel('rotate(-120deg) scale(0.7)', 'rotate(0deg) scale(1)') }} animate={{ opacity: 1, transform: 'rotate(0deg) scale(1)' }}
          transition={{ duration: 1.3, delay: at(bloom), ease: easeOut }}>
          {petals.map((petal, i) => <motion.ellipse key={`${petal.cx},${petal.cy}`} cx={petal.cx} cy={petal.cy} fill="currentColor"
            initial={calm ? false : { rx: 0, ry: 0 }} animate={{ rx: petal.rx, ry: petal.ry }}
            transition={{ duration: 0.7, delay: bloom + 0.1 + i * 0.08, ease: [0.34, 1.56, 0.64, 1] }} />)}
          <motion.circle className="heart" cx={32} cy={32} initial={calm ? false : { r: 0 }} animate={{ r: 5 }}
            transition={{ duration: 0.5, delay: bloom + 0.55, ease: [0.34, 1.9, 0.64, 1] }} />
        </motion.svg>
      </div>
      <h1>{words(t('settings.welcome.title_lead'), bloom + 0.25)} <em>{words(t('settings.welcome.title_accent'), bloom + 0.25 + lead * 0.07)}</em></h1>
      <motion.p {...enter(bloom + 0.75)}>{t('settings.welcome.lede')}</motion.p>
      <motion.button ref={button} className="primary" onClick={onContinue} {...enter(bloom + 0.95)}>{t('settings.welcome.continue_to_setup')}<ChevronRight size={15} /></motion.button>
    </main>
  </div>;
}
