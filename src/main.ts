import { mount } from 'svelte';
import App from './App.svelte';
import { applyCachedTheme } from '$lib/design/theme.svelte';
import './app.css';

// Before Svelte mounts, so the first frame is already the right colour rather
// than flashing a default light background at someone writing at night.
applyCachedTheme();

const target = document.getElementById('grimoire');
if (!target) throw new Error('Grimoire mount point is missing from index.html');

export default mount(App, { target });
