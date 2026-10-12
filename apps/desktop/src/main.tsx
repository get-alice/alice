import React from 'react';
import ReactDOM from 'react-dom/client';
import { MotionConfig } from 'motion/react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { setSystemLocales } from './i18n';
import App from './App';
import { wantsSystemMenu } from './ContextMenu';
import { snappy } from './motion';
import './styles.css';

// The WebView's own menu (Reload, Inspect) is not Alice's: outside text fields and selected text, a right-click
// shows Alice's menu for what was clicked, or nothing. Development builds keep it for Inspect.
if (!import.meta.env.DEV) document.addEventListener('contextmenu', event => { if (!wantsSystemMenu(event.target as Element)) event.preventDefault(); });

const root = document.getElementById('root');
if (!root) throw new Error('index.html is missing the #root element');
async function mount(element: HTMLElement) {
  if (isTauri()) {
    const language = await invoke<string>('system_locale');
    setSystemLocales([language]);
  } else setSystemLocales(navigator.languages);
  ReactDOM.createRoot(element).render(<React.StrictMode><MotionConfig reducedMotion="user" transition={snappy}><App /></MotionConfig></React.StrictMode>);
}
void mount(root);
