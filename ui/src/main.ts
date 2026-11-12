import 'monaco-editor/min/vs/editor/editor.main.css';
import '@fontsource/ibm-plex-sans/latin-400.css';
import '@fontsource/ibm-plex-sans/latin-500.css';
import '@fontsource/ibm-plex-sans/latin-600.css';
import '@fontsource/jetbrains-mono/latin-400.css';
import '@fontsource/jetbrains-mono/latin-400-italic.css';
import '@fontsource/jetbrains-mono/latin-500.css';
import '@fontsource/silkscreen/latin-400.css';
import './tokens.css';
import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

const target = document.getElementById('app') as HTMLElement;
const app = mount(App, {
  target,
  props: {
    onWorkspaceReady: () => {
      target.inert = false;
      document.getElementById('startup-splash')?.remove();
    }
  }
});

export default app;
