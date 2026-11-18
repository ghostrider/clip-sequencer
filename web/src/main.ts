import './fonts';
import './tokens.css';
import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { app } from './lib/app.svelte';
import { applyCachedTheme } from './lib/themes';

// The theme shown last in this browser, until the app sends the saved one (SPEC §9.7).
applyCachedTheme();
app.connect();
mount(App, { target: document.getElementById('app')! });
