import { mount } from 'svelte';

import './app.css';
import App from './App.svelte';

const target = document.getElementById('app');
if (target === null) {
  throw new Error('The page has no #app element to render into.');
}

mount(App, { target });
