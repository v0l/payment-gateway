import { render } from 'preact';
import '../tokens.css';
import './console.css';
import '../components.css';
import { App } from './App';
import { mountOverlays } from './dialogs';
import { legacyHashToPath } from './nav';

legacyHashToPath();
mountOverlays();
render(<App />, document.getElementById('root')!);
