import { mount } from 'svelte';

import App from './App.svelte';
import './tokens.css';
import './components.css';
import './styles.css';

// Theme and reduced motion are applied by the appearance store of `App` from the settings file
// (P2-00G); until then `tokens.css` follows the system (`:root:not([data-theme])`).

const target = document.getElementById('app') as HTMLElement;

// Synthetic drafts exist only in the DEMO build (`vite build --mode demo`). `MODE` is replaced at
// build time, so the normal build drops this branch and the demo module with it
// (`scripts/check-dist.mjs` verifies that).
if (import.meta.env.MODE === 'demo') {
  void Promise.all([import('./demo/drafts'), import('./demo/replace-host'), import('./demo/publication')]).then(
    ([{ createDemoDraftsClient }, { createDemoReplaceHostClient }, { createDemoPublicationClient }]) => {
      mount(App, {
        target,
        props: {
          draftsClient: createDemoDraftsClient(),
          replaceHostClient: createDemoReplaceHostClient(),
          publicationClient: createDemoPublicationClient(),
          demo: true
        }
      });
    }
  );
} else {
  mount(App, { target });
}
