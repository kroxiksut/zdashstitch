import { mount } from 'svelte';

import App from './App.svelte';
import { createThemeController } from './lib/theme';
import './tokens.css';
import './components.css';
import './styles.css';

createThemeController(
  document.documentElement,
  window.matchMedia('(prefers-color-scheme: dark)'),
  'system'
);

mount(App, {
  target: document.getElementById('app') as HTMLElement
});
