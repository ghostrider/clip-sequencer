import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-sans/600.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import '@fontsource/ibm-plex-mono/600.css';
import './tokens.css';
import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { app } from './lib/app.svelte';

app.connect();
mount(App, { target: document.getElementById('app')! });
