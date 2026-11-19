import { render } from 'preact';
import '../tokens.css';
import './sandbox.css';
import { SandboxApp } from './App';

render(<SandboxApp />, document.getElementById('root')!);
